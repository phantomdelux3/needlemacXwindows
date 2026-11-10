//! High-resolution album art and desktop backdrop capture.
//!
//! * `hires_art` – Windows only gets small thumbnails from players (Spotify
//!   300 px, browsers often less). We look the song up in the iTunes Search
//!   API (2000 px covers) with Deezer (1000 px) as fallback – both free, no key.
//! * `capture_backdrop` – grabs the desktop pixels behind the island window so
//!   the island can render real liquid glass (refraction of what's behind it).

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use crate::lyrics::{build_candidates, first_artist, norm};
use crate::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItunesItem {
    track_name: Option<String>,
    artist_name: Option<String>,
    collection_name: Option<String>,
    artwork_url100: Option<String>,
}
#[derive(Deserialize)]
struct ItunesResp {
    results: Vec<ItunesItem>,
}
#[derive(Deserialize)]
struct DeezerArtist {
    name: Option<String>,
}
#[derive(Deserialize)]
struct DeezerAlbum {
    title: Option<String>,
    cover_xl: Option<String>,
}
#[derive(Deserialize)]
struct DeezerItem {
    title: Option<String>,
    artist: Option<DeezerArtist>,
    album: Option<DeezerAlbum>,
}
#[derive(Deserialize)]
struct DeezerResp {
    data: Vec<DeezerItem>,
}

/// 0 = no match; higher = better. Title and artist must both match.
fn score(want_t: &str, want_a: &str, want_album: &str, t: &str, a: &str, album: &str) -> i32 {
    let (wt, wa, gt, ga) = (norm(want_t), norm(&first_artist(want_a)), norm(t), norm(a));
    if wt.is_empty() || gt.is_empty() {
        return 0;
    }
    let mut s = if gt == wt {
        6
    } else if gt.starts_with(&wt) || wt.starts_with(&gt) {
        4
    } else {
        return 0;
    };
    if !wa.is_empty() {
        if ga.contains(&wa) || wa.contains(&ga) {
            s += 4;
        } else {
            return 0;
        }
    }
    let (wal, gal) = (norm(want_album), norm(album));
    if !wal.is_empty() && !gal.is_empty() && (wal == gal || gal.starts_with(&wal) || wal.starts_with(&gal)) {
        s += 2;
    }
    s
}

async fn download(http: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let r = http.get(url).send().await.ok()?.error_for_status().ok()?;
    let b = r.bytes().await.ok()?;
    (b.len() > 2000).then(|| b.to_vec())
}

async fn from_itunes(http: &reqwest::Client, t: &str, a: &str, album: &str) -> Option<Vec<u8>> {
    let term = format!("{a} {t}");
    let resp: ItunesResp = http
        .get("https://itunes.apple.com/search")
        .query(&[("term", term.trim()), ("media", "music"), ("entity", "song"), ("limit", "20")])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let best = resp
        .results
        .iter()
        .map(|i| {
            let s = score(
                t,
                a,
                album,
                i.track_name.as_deref().unwrap_or(""),
                i.artist_name.as_deref().unwrap_or(""),
                i.collection_name.as_deref().unwrap_or(""),
            );
            (s, i)
        })
        .filter(|(s, i)| *s > 0 && i.artwork_url100.is_some())
        .max_by_key(|(s, _)| *s)?;
    let small = best.1.artwork_url100.as_deref()?;
    for size in ["2000x2000bb", "1400x1400bb", "1000x1000bb"] {
        let url = small.replace("100x100bb", size);
        if let Some(b) = download(http, &url).await {
            return Some(b);
        }
    }
    None
}

async fn from_deezer(http: &reqwest::Client, t: &str, a: &str, album: &str) -> Option<Vec<u8>> {
    let q = if a.is_empty() { format!("track:\"{t}\"") } else { format!("artist:\"{a}\" track:\"{t}\"") };
    let resp: DeezerResp = http
        .get("https://api.deezer.com/search")
        .query(&[("q", q.as_str()), ("limit", "15")])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let best = resp
        .data
        .iter()
        .map(|i| {
            let al = i.album.as_ref();
            let s = score(
                t,
                a,
                album,
                i.title.as_deref().unwrap_or(""),
                i.artist.as_ref().and_then(|x| x.name.as_deref()).unwrap_or(""),
                al.and_then(|x| x.title.as_deref()).unwrap_or(""),
            );
            (s, i)
        })
        .filter(|(s, i)| *s > 0 && i.album.as_ref().and_then(|x| x.cover_xl.as_ref()).is_some())
        .max_by_key(|(s, _)| *s)?;
    let url = best.1.album.as_ref()?.cover_xl.as_deref()?;
    download(http, url).await
}

#[tauri::command]
pub async fn hires_art(
    state: State<'_, AppState>,
    title: String,
    artist: String,
    album: String,
) -> Result<Option<String>, String> {
    let key = format!("{title}\u{1f}{artist}\u{1f}{album}");
    if let Some(hit) = state.art_cache.lock().unwrap().get(&key) {
        return Ok(hit.clone());
    }
    let http = state.http.clone();
    let mut found: Option<Vec<u8>> = None;
    for (t, a) in build_candidates(&title, &artist) {
        if a.is_empty() {
            continue; // never guess a cover from the title alone
        }
        found = from_itunes(&http, &t, &a, &album).await;
        if found.is_none() {
            found = from_deezer(&http, &t, &a, &album).await;
        }
        if found.is_some() {
            break;
        }
    }
    let url = found.map(|b| {
        let mime = if b.starts_with(&[0x89, b'P']) { "image/png" } else { "image/jpeg" };
        format!("data:{mime};base64,{}", STANDARD.encode(&b))
    });
    let mut cache = state.art_cache.lock().unwrap();
    if cache.len() > 24 {
        cache.clear();
    }
    cache.insert(key, url.clone());
    Ok(url)
}

/// JPEG of the screen area under the island window (physical pixels).
#[tauri::command(async)]
pub fn capture_backdrop(app: AppHandle) -> Result<String, String> {
    use windows::Win32::Graphics::Gdi::*;

    let win = app.get_webview_window("main").ok_or("no window")?;
    let pos = win.outer_position().map_err(|e| e.to_string())?;
    let size = win.outer_size().map_err(|e| e.to_string())?;
    let (w, h) = (size.width as i32, size.height as i32);
    if w <= 0 || h <= 0 {
        return Err("empty".into());
    }
    let mut buf = vec![0u8; (w * h * 4) as usize];
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp);
        let ok = BitBlt(mem, 0, 0, w, h, screen, pos.x, pos.y, SRCCOPY).is_ok();
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        if ok {
            GetDIBits(mem, bmp, 0, h as u32, Some(buf.as_mut_ptr() as _), &mut bi, DIB_RGB_COLORS);
        }
        SelectObject(mem, old);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if !ok {
            return Err("capture failed".into());
        }
    }
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for px in buf.chunks_exact(4) {
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 86)
        .encode(&rgb, w as u32, h as u32, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(format!("data:image/jpeg;base64,{}", STANDARD.encode(&out)))
}
