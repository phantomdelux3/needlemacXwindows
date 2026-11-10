//! Keeps the notch correct on every screen setup: any resolution, any Windows
//! scale (100 %…300 %+), several monitors with different scales, monitors being
//! plugged/unplugged, resolution or scale changes while running.
//!
//! * The island UI is designed for a 560×520 (CSS px) canvas. On small or
//!   heavily-scaled screens the whole island is zoomed down (webview zoom), so
//!   its layout – and every coordinate the UI reports – stays identical.
//! * A watcher re-applies the layout whenever the display setup changes.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize};

use crate::AppState;

pub const ISLAND_W: f64 = 560.0;
pub const ISLAND_H: f64 = 520.0;
/// Within this many logical px of the middle the notch snaps to the centre.
pub const MAGNET_PX: f64 = 46.0;

pub fn monitors(app: &AppHandle) -> Vec<Monitor> {
    app.available_monitors().unwrap_or_default()
}

/// The monitor the notch should live on (user choice → primary → first).
pub fn target_monitor(app: &AppHandle) -> Option<Monitor> {
    let want = app.state::<AppState>().settings.lock().unwrap().monitor.clone();
    let all = monitors(app);
    if !want.is_empty() {
        if let Some(m) = all.iter().find(|m| m.name().map(|n| *n == want).unwrap_or(false)) {
            return Some(m.clone());
        }
    }
    app.primary_monitor().ok().flatten().or_else(|| all.into_iter().next())
}

pub fn monitor_label(i: usize, m: &Monitor, primary: bool) -> String {
    let s = m.scale_factor();
    let size = m.size();
    format!(
        "Display {} – {}×{} @ {}%{}",
        i + 1,
        size.width,
        size.height,
        (s * 100.0).round(),
        if primary { " (main)" } else { "" }
    )
}

/// Zoom that makes the island fit the monitor (never enlarges past 100 %).
fn fit_zoom(m: &Monitor) -> f64 {
    let s = m.scale_factor();
    let (lw, lh) = (m.size().width as f64 / s, m.size().height as f64 / s);
    (lh * 0.92 / ISLAND_H).min(lw / ISLAND_W).clamp(0.45, 1.0)
}

/// Puts the island on the target monitor at `offset` logical px from the
/// top-centre, with a magnet at the middle. Returns the applied offset.
pub fn place_island(app: &AppHandle, offset: f64) -> f64 {
    let Some(win) = app.get_webview_window("main") else { return 0.0 };
    let Some(mon) = target_monitor(app) else { return 0.0 };
    let scale = mon.scale_factor();
    let zoom = *app.state::<AppState>().zoom.lock().unwrap();
    // window size in this monitor's physical pixels (don't trust outer_size
    // while the window may still be on a monitor with another scale)
    let ww = (ISLAND_W * zoom * scale).round();
    let (mp, ms) = (mon.position(), mon.size());
    let max = ((ms.width as f64 - ww) / 2.0 / scale).max(0.0);
    let mut off = offset.clamp(-max, max);
    if off.abs() < MAGNET_PX {
        off = 0.0;
    }
    let x = mp.x as f64 + (ms.width as f64 - ww) / 2.0 + off * scale;
    let _ = win.set_position(PhysicalPosition::new(x.round() as i32, mp.y));
    app.state::<AppState>().geo_dirty.store(true, Ordering::Relaxed);
    off
}

/// Size, zoom and position of the island for the current display setup;
/// also re-fits the full-screen view if it is open.
pub fn relayout(app: &AppHandle) {
    let Some(win) = app.get_webview_window("main") else { return };
    let Some(mon) = target_monitor(app) else { return };
    let zoom = fit_zoom(&mon);
    *app.state::<AppState>().zoom.lock().unwrap() = zoom;
    let off = app.state::<AppState>().settings.lock().unwrap().notch_offset;

    // move onto the monitor first (Windows rescales the window when it crosses
    // to a monitor with another DPI), then size, then centre precisely
    place_island(app, off);
    std::thread::sleep(Duration::from_millis(60));
    let s = mon.scale_factor();
    let _ = win.set_size(PhysicalSize::new(
        (ISLAND_W * zoom * s).round() as u32,
        (ISLAND_H * zoom * s).round() as u32,
    ));
    let _ = win.set_zoom(zoom);
    std::thread::sleep(Duration::from_millis(40));
    place_island(app, off);

    if app
        .get_webview_window("full")
        .and_then(|f| f.is_visible().ok())
        .unwrap_or(false)
    {
        fit_full(app);
    }
    let _ = tauri::Emitter::emit(app, "display", zoom);
}

/// Make the full-screen window cover exactly the notch's monitor. Re-asserts a
/// moment later because crossing to a monitor with another DPI makes Windows
/// resize the window on its own.
pub fn fit_full(app: &AppHandle) {
    let Some(full) = app.get_webview_window("full") else { return };
    let Some(mon) = target_monitor(app) else { return };
    let (pos, size) = (*mon.position(), *mon.size());
    let apply = |f: &tauri::WebviewWindow| {
        let _ = f.set_position(pos);
        let _ = f.set_size(size);
    };
    apply(&full);
    let f2 = full.clone();
    std::thread::spawn(move || {
        for wait in [120u64, 350, 900] {
            std::thread::sleep(Duration::from_millis(wait));
            let ok = f2.outer_position().map(|p| p == pos).unwrap_or(false)
                && f2.outer_size().map(|s| s == size).unwrap_or(false);
            if !ok {
                let _ = f2.set_position(pos);
                let _ = f2.set_size(size);
            }
        }
    });
}

fn signature(app: &AppHandle) -> String {
    let want = app.state::<AppState>().settings.lock().unwrap().monitor.clone();
    let prim = app.primary_monitor().ok().flatten().and_then(|m| m.name().cloned()).unwrap_or_default();
    let mut s = format!("{want}|{prim}|");
    for m in monitors(app) {
        let (p, z) = (m.position(), m.size());
        s += &format!("{}:{},{},{}x{}@{};", m.name().cloned().unwrap_or_default(), p.x, p.y, z.width, z.height, m.scale_factor());
    }
    s
}

fn names(app: &AppHandle) -> Vec<String> {
    monitors(app).iter().map(|m| m.name().cloned().unwrap_or_default()).collect()
}

/// Watches for display changes (plug/unplug, resolution, scale, primary).
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = String::new();
        let mut last_names: Vec<String> = Vec::new();
        loop {
            let sig = signature(&app);
            if sig != last {
                last = sig;
                relayout(&app);
                let n = names(&app);
                if n != last_names {
                    last_names = n;
                    crate::rebuild_menu(&app);
                }
            }
            std::thread::sleep(Duration::from_millis(1200));
        }
    });
}

/// Drag the notch sideways with the mouse. Tracking happens here (real screen
/// pixels) so it is exact at any scale, and ends by itself when the left
/// button is released.
pub fn drag(app: AppHandle) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let state = app.state::<AppState>();
    if state.dragging.swap(true, Ordering::SeqCst) {
        return;
    }
    let from = state.settings.lock().unwrap().notch_offset;
    std::thread::spawn(move || {
        let scale = target_monitor(&app).map(|m| m.scale_factor()).unwrap_or(1.0);
        let mut start = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut start);
        }
        let mut applied = from;
        let mut last_x = i32::MIN;
        loop {
            std::thread::sleep(Duration::from_millis(8));
            let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000 != 0;
            if !down {
                break;
            }
            let mut p = POINT::default();
            if unsafe { GetCursorPos(&mut p) }.is_err() || p.x == last_x {
                continue;
            }
            last_x = p.x;
            let want = from + (p.x - start.x) as f64 / scale;
            applied = place_island(&app, want);
            let _ = tauri::Emitter::emit(&app, "notch:drag", serde_json::json!({ "applied": applied, "want": want, "done": false }));
        }
        app.state::<AppState>().dragging.store(false, Ordering::SeqCst);
        crate::update_setting(&app, |s| s.notch_offset = applied);
        let _ = tauri::Emitter::emit(&app, "notch:drag", serde_json::json!({ "applied": applied, "want": applied, "done": true }));
    });
}
