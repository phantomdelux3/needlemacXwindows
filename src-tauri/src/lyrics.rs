//! Time-synced lyrics from LRCLIB (https://lrclib.net) – free, no key needed.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

#[derive(Serialize, Clone)]
pub struct LyricLine {
    pub time: f64,
    pub text: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LyricsResult {
    pub found: bool,
    pub instrumental: bool,
    pub synced: Vec<LyricLine>,
    pub plain: Option<String>,
    pub matched_title: Option<String>,
    pub matched_artist: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Item {
    track_name: Option<String>,
    artist_name: Option<String>,
    duration: Option<f64>,
    instrumental: Option<bool>,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

const API: &str = "https://lrclib.net/api";

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("NeedleX/0.1 (desktop lyrics island for Windows)")
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .expect("http client")
}

#[tauri::command]
pub async fn fetch_lyrics(
    state: State<'_, AppState>,
    title: String,
    artist: String,
    duration_ms: f64,
) -> Result<LyricsResult, String> {
    let dur = duration_ms / 1000.0;
    let cache_key = format!("{title}\u{1f}{artist}\u{1f}{}", (dur / 3.0).round());
    if let Some(hit) = state.lyrics_cache.lock().unwrap().get(&cache_key) {
        return Ok(hit.clone());
    }
    let http = state.http.clone();

    let candidates = build_candidates(&title, &artist);
    let mut best: Option<(i32, Item)> = None;
    let mut net_ok = false;

    // 1) structured search per candidate
    for (t, a) in &candidates {
        let mut q: Vec<(&str, &str)> = vec![("track_name", t.as_str())];
        if !a.is_empty() {
            q.push(("artist_name", a.as_str()));
        }
        if let Ok(items) = search(&http, &q).await {
            net_ok = true;
            consider(items, t, dur, &mut best);
        }
        if matches!(&best, Some((s, _)) if *s >= 200) {
            break;
        }
    }
    // 2) free-text fallback
    if !matches!(&best, Some((s, _)) if *s >= 150) {
        for (t, a) in &candidates {
            let q = format!("{a} {t}");
            if let Ok(items) = search(&http, &[("q", q.trim())]).await {
                net_ok = true;
                consider(items, t, dur, &mut best);
            }
            if matches!(&best, Some((s, _)) if *s >= 200) {
                break;
            }
        }
    }

    if !net_ok {
        return Err("lyrics service unreachable".into());
    }

    let result = match best {
        Some((_, item)) => {
            let synced = item
                .synced_lyrics
                .as_deref()
                .map(parse_lrc)
                .unwrap_or_default();
            let instrumental = item.instrumental.unwrap_or(false);
            LyricsResult {
                found: !synced.is_empty() || item.plain_lyrics.is_some() || instrumental,
                instrumental,
                synced,
                plain: item.plain_lyrics.filter(|p| !p.trim().is_empty()),
                matched_title: item.track_name,
                matched_artist: item.artist_name,
            }
        }
        None => LyricsResult::default(),
    };
    state
        .lyrics_cache
        .lock()
        .unwrap()
        .insert(cache_key, result.clone());
    Ok(result)
}

async fn search(http: &reqwest::Client, q: &[(&str, &str)]) -> Result<Vec<Item>, reqwest::Error> {
    http.get(format!("{API}/search"))
        .query(q)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<Item>>()
        .await
}

fn consider(items: Vec<Item>, wanted_title: &str, dur: f64, best: &mut Option<(i32, Item)>) {
    let want = norm(wanted_title);
    for item in items {
        let mut score = 0;
        let has_synced = item.synced_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty());
        if has_synced {
            score += 100;
        } else if item.plain_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty()) {
            score += 40;
        } else if item.instrumental == Some(true) {
            score += 30;
        } else {
            continue;
        }
        if let (true, Some(d)) = (dur > 1.0, item.duration) {
            let diff = (d - dur).abs();
            score += if diff <= 2.0 {
                60
            } else if diff <= 5.0 {
                35
            } else if diff <= 10.0 {
                10
            } else if diff <= 20.0 {
                -40
            } else {
                -300
            };
        }
        let got = norm(item.track_name.as_deref().unwrap_or(""));
        if !want.is_empty() && !got.is_empty() {
            if got == want {
                score += 40;
            } else if got.contains(&want) || want.contains(&got) {
                score += 20;
            } else {
                score -= 60;
            }
        }
        if score > 0 && best.as_ref().map_or(true, |(s, _)| score > *s) {
            *best = Some((score, item));
        }
    }
}

pub(crate) fn norm(s: &str) -> String {
    clean_title(s)
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

static BRACKETS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\s*[\(\[\{【「][^\)\]\}】」]*?(official|video|audio|lyric|visuali[sz]er|m/?v\b|\bhd\b|\bhq\b|4k|remaster|explicit|clean|feat|ft\.|with |prod|version|edit|mix|live|from|soundtrack|ost)[^\)\]\}】」]*[\)\]\}】」]",
    )
    .unwrap()
});
static FEAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s+(feat\.?|ft\.?|featuring)\s+.*$").unwrap());
static DASH_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s+[-–—]\s+[^-–—]*(remaster|version|edit|mix|live|mono|stereo|bonus|demo)[^-–—]*$")
        .unwrap()
});
static PIPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[|｜].*$").unwrap());
static LRC_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(\d{1,3}):(\d{1,2}(?:[.:]\d{1,3})?)\]").unwrap());
static WORD_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<\d{1,3}:\d{1,2}(?:[.:]\d{1,3})?>").unwrap());

pub(crate) fn clean_title(t: &str) -> String {
    let mut s = PIPE.replace(t, "").to_string();
    for _ in 0..3 {
        s = BRACKETS.replace_all(&s, "").to_string();
    }
    s = DASH_SUFFIX.replace(&s, "").to_string();
    s = FEAT.replace(&s, "").to_string();
    s.trim().trim_matches(|c| c == '"' || c == '\'' || c == '“' || c == '”').trim().to_string()
}

pub(crate) fn clean_artist(a: &str) -> String {
    let mut s = a.trim().to_string();
    for suffix in [" - Topic", "VEVO", "Vevo", " Official"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.trim().to_string();
        }
    }
    s
}

pub(crate) fn first_artist(a: &str) -> String {
    let lower = a.to_lowercase();
    let mut cut = a.len();
    for sep in [", ", " & ", " x ", " feat", " ft.", ";", " / ", " and ", "、"] {
        if let Some(i) = lower.find(sep) {
            cut = cut.min(i);
        }
    }
    a[..cut].trim().to_string()
}

/// (title, artist) pairs to try, most likely first.
pub(crate) fn build_candidates(title: &str, artist: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |t: String, a: String| {
        if !t.is_empty() && !out.iter().any(|(ot, oa)| *ot == t && *oa == a) {
            out.push((t, a));
        }
    };
    let artist_c = clean_artist(artist);
    let title_c = clean_title(title);

    // YouTube style "Artist - Song (Official Video)" where artist = channel name.
    // Split the *cleaned* title so "Yellow - Remastered 2011" isn't split.
    for sep in [" - ", " – ", " — "] {
        if let Some((left, right)) = title_c.split_once(sep) {
            let (l, r) = (clean_artist(left.trim()), clean_title(right));
            push(r.clone(), first_artist(&l));
            push(title_c.clone(), first_artist(&artist_c));
            break;
        }
    }
    push(title_c.clone(), first_artist(&artist_c));
    if artist_c != first_artist(&artist_c) {
        push(title_c.clone(), artist_c.clone());
    }
    // "School Fight (Lost Tapes 2020)" → "School Fight": drop any remaining extras
    let bare = ANY_BRACKETS.replace_all(&title_c, "").trim().to_string();
    if !bare.is_empty() && bare != title_c {
        push(bare, first_artist(&artist_c));
    }
    push(title_c, String::new());
    out
}

static ANY_BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*[\(\[\{【「][^\)\]\}】」]*[\)\]\}】」]").unwrap());

fn parse_lrc(s: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in s.lines() {
        let times: Vec<f64> = LRC_TAG
            .captures_iter(raw)
            .filter_map(|c| {
                let m: f64 = c[1].parse().ok()?;
                let sec: f64 = c[2].replace(':', ".").parse().ok()?;
                Some((m * 60.0 + sec) * 1000.0)
            })
            .collect();
        if times.is_empty() {
            continue;
        }
        let text = LRC_TAG.replace_all(raw, "");
        let text = WORD_TAG.replace_all(&text, "").trim().to_string();
        for t in times {
            lines.push(LyricLine { time: t, text: text.clone() });
        }
    }
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_youtube_titles() {
        assert_eq!(clean_title("Blinding Lights (Official Video)"), "Blinding Lights");
        assert_eq!(clean_title("Song Name [Official Music Video] | 4K"), "Song Name");
        assert_eq!(clean_title("Yellow - Remastered 2011"), "Yellow");
        assert_eq!(clean_title("Hello feat. Someone"), "Hello");
        let c = build_candidates("The Weeknd - Blinding Lights (Official Video)", "TheWeekndVEVO");
        assert_eq!(c[0], ("Blinding Lights".to_string(), "The Weeknd".to_string()));
    }

    #[test]
    fn parses_lrc() {
        let l = parse_lrc("[00:12.50]Hello\n[01:02.00][01:30.10]Chorus <00:01.00>word\n[ar:x]");
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].time, 12500.0);
        assert_eq!(l[1].text, "Chorus word");
        assert_eq!(l[2].time, 90100.0);
    }
}
