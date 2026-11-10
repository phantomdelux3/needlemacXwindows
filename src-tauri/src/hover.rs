//! The island window is mostly transparent. It is click-through everywhere
//! except over the island itself; we poll the global cursor to detect hover
//! (the webview can't see the mouse while the window ignores it).

use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

use crate::AppState;

#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

fn hwnd_of(win: &WebviewWindow) -> Option<HWND> {
    win.hwnd().ok().map(|h| HWND(h.0 as _))
}

/// Never steal focus from the app you're using, hide from Alt+Tab, and
/// optionally let clicks fall through to whatever is underneath.
fn apply_style(hwnd: HWND, click_through: bool) {
    unsafe {
        let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let mut want = cur | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        let ct = (WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0) as isize;
        if click_through {
            want |= ct;
        } else {
            want &= !ct;
        }
        if want != cur {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
        }
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut inside = false;
        let mut geo: Option<(f64, f64, f64)> = None; // (x, y, scale)
        let mut geo_at = Instant::now() - Duration::from_secs(10);
        loop {
            std::thread::sleep(Duration::from_millis(30));
            let Some(win) = app.get_webview_window("main") else { continue };
            let Some(hwnd) = hwnd_of(&win) else { continue };

            let state = app.state::<AppState>();
            let dirty = state.geo_dirty.swap(false, std::sync::atomic::Ordering::Relaxed);
            if dirty || geo.is_none() || geo_at.elapsed() > Duration::from_millis(1500) {
                if let (Ok(p), Ok(s)) = (win.outer_position(), win.scale_factor()) {
                    geo = Some((p.x as f64, p.y as f64, s));
                    geo_at = Instant::now();
                }
            }
            let Some((wx, wy, scale)) = geo else { continue };
            let r = *state.hit.lock().unwrap();

            let mut pt = POINT::default();
            if unsafe { GetCursorPos(&mut pt) }.is_err() {
                continue;
            }
            let (px, py) = (pt.x as f64, pt.y as f64);
            let x0 = wx + r.x * scale;
            let y0 = wy + r.y * scale;
            // while the notch is dragged the window must keep the mouse
            let dragging = state.dragging.load(std::sync::atomic::Ordering::Relaxed);
            let now_inside = dragging || r.w > 0.0
                && px >= x0
                && px <= x0 + r.w * scale
                && py >= y0 - 2.0 // include the very top pixel row of the screen
                && py <= y0 + r.h * scale;

            apply_style(hwnd, !now_inside);
            if now_inside != inside {
                inside = now_inside;
                let _ = app.emit("island:hover", inside);
            }
        }
    });
}
