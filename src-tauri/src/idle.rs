//! Opens the full-screen view automatically when the PC has been idle (no
//! mouse/keyboard input) for the chosen number of minutes while music plays.
//! It stays out of the way of games, full-screen video and presentations.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};

use crate::AppState;

fn idle_ms() -> u64 {
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if !GetLastInputInfo(&mut li).as_bool() {
            return 0;
        }
        GetTickCount().wrapping_sub(li.dwTime) as u64
    }
}

/// false while a game / full-screen video / presentation / lock screen is up
fn screen_is_free() -> bool {
    match unsafe { SHQueryUserNotificationState() } {
        Ok(s) => !(s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE || s == QUNS_NOT_PRESENT),
        Err(_) => true,
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut fired = false;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let state = app.state::<AppState>();
            let minutes = state.settings.lock().unwrap().auto_full_minutes;
            let idle = idle_ms();
            if idle < 4000 {
                fired = false; // user is back – arm again for the next idle period
            }
            if minutes == 0 || fired {
                continue;
            }
            if idle < minutes as u64 * 60_000 || !state.playing.load(Ordering::Relaxed) {
                continue;
            }
            let full_open = app
                .get_webview_window("full")
                .and_then(|f| f.is_visible().ok())
                .unwrap_or(false);
            if full_open || !screen_is_free() {
                continue;
            }
            fired = true;
            crate::show_full(&app, true);
        }
    });
}
