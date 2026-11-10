//! Watches Windows' System Media Transport Controls (SMTC).
//!
//! Every app that shows up in the Windows volume / media flyout reports here:
//! Spotify, Apple Music, Chrome/Edge/Firefox/Brave/Opera tabs (YouTube Music,
//! Spotify Web, SoundCloud, YouTube...), Media Player, VLC, foobar2000, etc.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager as _};

use crate::AppState;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Props,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};

pub enum MediaCmd {
    Toggle,
    Next,
    Prev,
    Seek(f64),
    Resend,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: Option<String>,
    pub source: String,
    pub source_name: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlayState {
    pub active: bool,
    pub playing: bool,
    pub position_ms: f64,
    pub duration_ms: f64,
    pub has_timeline: bool,
    pub can_next: bool,
    pub can_prev: bool,
    pub can_seek: bool,
}

pub fn spawn(app: AppHandle, rx: Receiver<MediaCmd>) {
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(
                windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
            );
        }
        let mut w = Worker {
            app,
            mgr: None,
            current_id: String::new(),
            track: Track::default(),
            art_hash: 0,
            art_checks: 0,
        };
        loop {
            match rx.recv_timeout(Duration::from_millis(400)) {
                Ok(cmd) => {
                    w.exec(cmd);
                    w.tick();
                }
                Err(RecvTimeoutError::Timeout) => w.tick(),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

struct Worker {
    app: AppHandle,
    mgr: Option<Manager>,
    current_id: String,
    track: Track,
    art_hash: u64,
    art_checks: u8,
}

impl Worker {
    fn manager(&mut self) -> Option<&Manager> {
        if self.mgr.is_none() {
            self.mgr = Manager::RequestAsync().and_then(|op| op.get()).ok();
        }
        self.mgr.as_ref()
    }

    /// Prefer: the session we already follow if it is playing → any playing
    /// session → the session we already follow → whatever Windows calls current.
    fn pick_session(&mut self) -> Option<Session> {
        let current_id = self.current_id.clone();
        let mgr = self.manager()?;
        let sessions = mgr.GetSessions().ok()?;
        let n = sessions.Size().unwrap_or(0);
        let mut first_playing: Option<Session> = None;
        let mut same: Option<(Session, bool)> = None;
        for i in 0..n {
            let Ok(s) = sessions.GetAt(i) else { continue };
            let id = s
                .SourceAppUserModelId()
                .map(|h| h.to_string())
                .unwrap_or_default();
            let playing = s
                .GetPlaybackInfo()
                .and_then(|p| p.PlaybackStatus())
                .map(|st| st == Status::Playing)
                .unwrap_or(false);
            if id == current_id {
                same = Some((s.clone(), playing));
            }
            if playing && first_playing.is_none() {
                first_playing = Some(s);
            }
        }
        if let Some((s, true)) = &same {
            return Some(s.clone());
        }
        if first_playing.is_some() {
            return first_playing;
        }
        if let Some((s, _)) = same {
            return Some(s);
        }
        mgr.GetCurrentSession().ok()
    }

    fn exec(&mut self, cmd: MediaCmd) {
        if let MediaCmd::Resend = cmd {
            self.emit_track();
            return;
        }
        let Some(s) = self.pick_session() else { return };
        let _ = (|| -> windows::core::Result<()> {
            match cmd {
                MediaCmd::Toggle => {
                    s.TryTogglePlayPauseAsync()?.get()?;
                }
                MediaCmd::Next => {
                    s.TrySkipNextAsync()?.get()?;
                }
                MediaCmd::Prev => {
                    s.TrySkipPreviousAsync()?.get()?;
                }
                MediaCmd::Seek(ms) => {
                    let start = s
                        .GetTimelineProperties()
                        .and_then(|t| t.StartTime())
                        .map(|t| t.Duration)
                        .unwrap_or(0);
                    s.TryChangePlaybackPositionAsync(start + (ms * 10_000.0) as i64)?
                        .get()?;
                }
                MediaCmd::Resend => {}
            }
            Ok(())
        })();
    }

    fn tick(&mut self) {
        let result = match self.pick_session() {
            Some(s) => self.update(&s),
            None => Ok(false),
        };
        match result {
            Ok(true) => {}
            Ok(false) => self.clear(),
            Err(_) => {
                // Manager can go stale (e.g. explorer restart) – recreate next tick.
                self.mgr = None;
                self.clear();
            }
        }
    }

    fn clear(&mut self) {
        if !self.track.key.is_empty() {
            self.track = Track::default();
            self.emit_track();
        }
        self.app.state::<AppState>().playing.store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = self.app.emit("media:state", PlayState::default());
    }

    /// Returns Ok(false) when the session has no usable metadata.
    fn update(&mut self, s: &Session) -> windows::core::Result<bool> {
        let id = s.SourceAppUserModelId()?.to_string();
        self.current_id = id.clone();

        let props = s.TryGetMediaPropertiesAsync()?.get()?;
        let title = props.Title()?.to_string();
        let mut artist = props.Artist()?.to_string();
        if artist.is_empty() {
            artist = props.AlbumArtist().map(|a| a.to_string()).unwrap_or_default();
        }
        let album = props.AlbumTitle()?.to_string();
        if title.trim().is_empty() {
            return Ok(false);
        }

        let key = format!("{id}\u{1f}{title}\u{1f}{artist}\u{1f}{album}");
        if key != self.track.key {
            self.track = Track {
                key,
                title,
                artist,
                album,
                art: None,
                source: id.clone(),
                source_name: friendly_name(&id),
            };
            self.art_hash = 0;
            // Browsers sometimes publish the artwork a moment after the title,
            // so keep re-checking the thumbnail for a few seconds.
            self.art_checks = 10;
            self.check_art(&props);
            self.emit_track();
        } else if self.art_checks > 0 {
            self.art_checks -= 1;
            if self.check_art(&props) {
                self.emit_track();
            }
        }

        let info = s.GetPlaybackInfo()?;
        let playing = info.PlaybackStatus()? == Status::Playing;
        self.app.state::<AppState>().playing.store(playing, std::sync::atomic::Ordering::Relaxed);
        let (can_next, can_prev, can_seek) = info
            .Controls()
            .map(|c| {
                (
                    c.IsNextEnabled().unwrap_or(true),
                    c.IsPreviousEnabled().unwrap_or(true),
                    c.IsPlaybackPositionEnabled().unwrap_or(false),
                )
            })
            .unwrap_or((true, true, false));

        let tl = s.GetTimelineProperties()?;
        let start = tl.StartTime()?.Duration;
        let end = tl.EndTime()?.Duration;
        let pos = tl.Position()?.Duration;
        let updated = tl.LastUpdatedTime()?.UniversalTime;

        let duration_ms = (end - start).max(0) as f64 / 10_000.0;
        let mut position_ms = (pos - start).max(0) as f64 / 10_000.0;
        // SMTC gives a position snapshot taken at `updated`; extrapolate to now.
        if playing && updated > 0 {
            let delta = now_filetime() - updated;
            if delta > 0 && delta < 36_000_000_000 {
                position_ms += delta as f64 / 10_000.0;
            }
        }
        if duration_ms > 0.0 {
            position_ms = position_ms.clamp(0.0, duration_ms);
        }

        let state = PlayState {
            active: true,
            playing,
            position_ms,
            duration_ms,
            has_timeline: duration_ms > 1000.0,
            can_next,
            can_prev,
            can_seek,
        };
        let _ = self.app.emit("media:state", state);
        Ok(true)
    }

    fn check_art(&mut self, props: &Props) -> bool {
        let Ok(thumb) = props.Thumbnail() else { return false };
        let Ok(bytes) = read_stream(&thumb) else { return false };
        if bytes.len() < 64 {
            return false;
        }
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        let h = h.finish();
        if h == self.art_hash {
            return false;
        }
        self.art_hash = h;
        let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            "image/png"
        } else if bytes.starts_with(&[0xFF, 0xD8]) {
            "image/jpeg"
        } else if bytes.starts_with(b"RIFF") {
            "image/webp"
        } else if bytes.starts_with(b"GIF") {
            "image/gif"
        } else if bytes.starts_with(b"BM") {
            "image/bmp"
        } else {
            "image/png"
        };
        self.track.art = Some(format!("data:{mime};base64,{}", STANDARD.encode(&bytes)));
        true
    }

    fn emit_track(&self) {
        let _ = self.app.emit("media:track", self.track.clone());
    }
}

fn read_stream(r: &IRandomAccessStreamReference) -> windows::core::Result<Vec<u8>> {
    let stream = r.OpenReadAsync()?.get()?;
    let size = stream.Size()? as u32;
    if size == 0 {
        return Ok(vec![]);
    }
    let input = stream.GetInputStreamAt(0)?;
    let reader = DataReader::CreateDataReader(&input)?;
    reader.LoadAsync(size)?.get()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf)?;
    Ok(buf)
}

/// Current time as a Windows FILETIME (100 ns ticks since 1601-01-01 UTC).
fn now_filetime() -> i64 {
    let unix_100ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() / 100)
        .unwrap_or(0) as i64;
    unix_100ns + 116_444_736_000_000_000
}

fn friendly_name(id: &str) -> String {
    let l = id.to_lowercase();
    let known = [
        ("spotify", "Spotify"),
        ("applemusic", "Apple Music"),
        ("appleinc", "Apple Music"),
        ("itunes", "iTunes"),
        ("msedge", "Edge"),
        ("chrome", "Chrome"),
        ("firefox", "Firefox"),
        ("brave", "Brave"),
        ("opera", "Opera"),
        ("vivaldi", "Vivaldi"),
        ("arc", "Arc"),
        ("zunemusic", "Media Player"),
        ("zunevideo", "Movies & TV"),
        ("vlc", "VLC"),
        ("foobar", "foobar2000"),
        ("musicbee", "MusicBee"),
        ("aimp", "AIMP"),
        ("tidal", "TIDAL"),
        ("deezer", "Deezer"),
        ("amazon", "Amazon Music"),
        ("youtube", "YouTube Music"),
        ("soundcloud", "SoundCloud"),
    ];
    for (needle, name) in known {
        if l.contains(needle) {
            return name.to_string();
        }
    }
    let base = id.split('!').next_back().unwrap_or(id);
    let base = base.trim_end_matches(".exe").trim_end_matches(".EXE");
    let base = base.rsplit(['\\', '/']).next().unwrap_or(base);
    if base.is_empty() {
        "Media".into()
    } else {
        base.to_string()
    }
}
