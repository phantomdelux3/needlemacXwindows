#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod art;
mod audio;
mod hover;
mod idle;
mod lyrics;
mod media;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindowBuilder, Wry,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use media::MediaCmd;

/// Auto full-screen choices offered in the tray (minutes, 0 = off).
const IDLE_CHOICES: [u32; 6] = [0, 1, 3, 5, 10, 15];
/// Within this many px of the middle the notch snaps back to the centre.
const MAGNET_PX: f64 = 46.0;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub floating: bool,
    pub live_lyrics: bool,
    pub hide_idle: bool,
    pub glow: bool,
    /// Open the full-screen view after this many idle minutes (0 = off).
    pub auto_full_minutes: u32,
    /// Notch hidden from the screen (bring it back from the tray).
    pub notch_hidden: bool,
    /// Horizontal offset of the notch from the screen centre, logical px.
    pub notch_offset: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            floating: false,
            live_lyrics: true,
            hide_idle: true,
            glow: true,
            auto_full_minutes: 3,
            notch_hidden: false,
            notch_offset: 0.0,
        }
    }
}

pub struct AppState {
    pub media_tx: Mutex<Sender<MediaCmd>>,
    pub hit: Mutex<hover::Rect>,
    pub lyrics_cache: Mutex<HashMap<String, lyrics::LyricsResult>>,
    pub art_cache: Mutex<HashMap<String, Option<String>>>,
    pub http: reqwest::Client,
    pub settings: Mutex<Settings>,
    /// Something is playing right now (set by the media watcher).
    pub playing: AtomicBool,
    /// The island window moved – hover detection must re-read its position.
    pub geo_dirty: AtomicBool,
    /// The notch is being dragged – keep the window interactive.
    pub dragging: AtomicBool,
    pub tray_items: Mutex<HashMap<String, CheckMenuItem<Wry>>>,
}

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, s: &Settings) {
    if let Some(p) = settings_path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(s).unwrap_or_default());
    }
}

// ---------- commands ----------

#[tauri::command]
fn media_control(state: State<AppState>, action: String) {
    let cmd = match action.as_str() {
        "toggle" => MediaCmd::Toggle,
        "next" => MediaCmd::Next,
        "prev" => MediaCmd::Prev,
        _ => return,
    };
    let _ = state.media_tx.lock().unwrap().send(cmd);
}

#[tauri::command]
fn media_seek(state: State<AppState>, ms: f64) {
    let _ = state.media_tx.lock().unwrap().send(MediaCmd::Seek(ms.max(0.0)));
}

#[tauri::command]
fn media_resend(state: State<AppState>) {
    let _ = state.media_tx.lock().unwrap().send(MediaCmd::Resend);
}

#[tauri::command]
fn set_hit_rect(state: State<AppState>, x: f64, y: f64, w: f64, h: f64) {
    *state.hit.lock().unwrap() = hover::Rect { x, y, w, h };
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn open_full(app: AppHandle) {
    show_full(&app, false);
}

#[tauri::command]
fn close_full(app: AppHandle) {
    if let Some(full) = app.get_webview_window("full") {
        let _ = full.hide();
    }
    let hidden = app.state::<AppState>().settings.lock().unwrap().notch_hidden;
    if let (Some(main), false) = (app.get_webview_window("main"), hidden) {
        let _ = main.show();
    }
}

/// Hide / show the notch (button in the island, or the tray).
#[tauri::command]
fn set_notch_hidden(app: AppHandle, hidden: bool) {
    apply_notch_hidden(&app, hidden);
}

/// Drag the notch sideways. `offset` is logical px from the screen centre.
/// Returns the applied offset (0 when the centre magnet caught it).
#[tauri::command]
fn move_notch(app: AppHandle, state: State<AppState>, offset: f64, done: bool) -> f64 {
    let applied = place_island(&app, offset);
    if done {
        state.dragging.store(false, Ordering::Relaxed);
        update_setting(&app, |s| s.notch_offset = applied);
    } else {
        state.dragging.store(true, Ordering::Relaxed);
    }
    applied
}

// ---------- window / tray ----------

/// Opens the full-screen view on the island's monitor. `auto` = idle timer.
pub fn show_full(app: &AppHandle, auto: bool) {
    let Some(full) = app.get_webview_window("full") else { return };
    if let Some(main) = app.get_webview_window("main") {
        if let Ok(Some(m)) = main.current_monitor() {
            let _ = full.set_position(*m.position());
            let _ = full.set_size(*m.size());
        }
        let _ = main.hide();
    }
    let _ = full.show();
    let _ = full.set_focus();
    let _ = app.emit_to("full", "full:open", serde_json::json!({ "auto": auto }));
    let _ = app.state::<AppState>().media_tx.lock().unwrap().send(MediaCmd::Resend);
}

fn apply_notch_hidden(app: &AppHandle, hidden: bool) {
    update_setting(app, |s| s.notch_hidden = hidden);
    let full_open = app
        .get_webview_window("full")
        .and_then(|f| f.is_visible().ok())
        .unwrap_or(false);
    if let Some(main) = app.get_webview_window("main") {
        if hidden {
            let _ = main.hide();
        } else if !full_open {
            let _ = main.show();
        }
    }
    if let Some(i) = app.state::<AppState>().tray_items.lock().unwrap().get("hide_notch") {
        let _ = i.set_checked(hidden);
    }
}

/// Positions the island window at `offset` logical px from the top-centre of
/// its monitor, with a magnet at the middle. Returns the applied offset.
fn place_island(app: &AppHandle, offset: f64) -> f64 {
    let Some(win) = app.get_webview_window("main") else { return 0.0 };
    let mon = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    let Some(mon) = mon else { return 0.0 };
    let Ok(ws) = win.outer_size() else { return 0.0 };
    let scale = mon.scale_factor();
    let (mp, ms) = (mon.position(), mon.size());
    let max = ((ms.width as f64 - ws.width as f64) / 2.0 / scale).max(0.0);
    let mut off = offset.clamp(-max, max);
    if off.abs() < MAGNET_PX {
        off = 0.0;
    }
    let x = mp.x as f64 + (ms.width as f64 - ws.width as f64) / 2.0 + off * scale;
    let _ = win.set_position(PhysicalPosition::new(x.round() as i32, mp.y));
    app.state::<AppState>().geo_dirty.store(true, Ordering::Relaxed);
    off
}

fn position_window(app: &AppHandle) {
    let off = app.state::<AppState>().settings.lock().unwrap().notch_offset;
    place_island(app, off);
}

fn update_setting(app: &AppHandle, f: impl FnOnce(&mut Settings)) {
    let state = app.state::<AppState>();
    let s = {
        let mut s = state.settings.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    save_settings(app, &s);
    let _ = app.emit("settings", s);
}

fn idle_label(m: u32) -> String {
    match m {
        0 => "Off".into(),
        1 => "After 1 minute".into(),
        m => format!("After {m} minutes"),
    }
}

fn build_tray(app: &AppHandle, s: &Settings) -> tauri::Result<()> {
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let check = |id: &str, label: &str, on: bool| CheckMenuItemBuilder::with_id(id, label).checked(on).build(app);

    let hide_notch = check("hide_notch", "Hide notch", s.notch_hidden)?;
    let floating = check("floating", "Floating island (pill shape)", s.floating)?;
    let live = check("live", "Live lyrics under the island", s.live_lyrics)?;
    let hide = check("hide", "Hide when nothing is playing", s.hide_idle)?;
    let glow = check("glow", "Beat glow", s.glow)?;
    let auto = check("autostart", "Launch at startup", autostart_on)?;

    let mut idle_items = Vec::new();
    let mut idle_menu = SubmenuBuilder::new(app, "Full screen when idle");
    for m in IDLE_CHOICES {
        let it = check(&format!("idle_{m}"), &idle_label(m), s.auto_full_minutes == m)?;
        idle_menu = idle_menu.item(&it);
        idle_items.push(it);
    }
    let idle_menu = idle_menu.build()?;

    let open_full_item = MenuItemBuilder::with_id("open_full", "Open full screen").build(app)?;
    let recenter = MenuItemBuilder::with_id("recenter", "Re-center notch").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit NeedleX").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[&hide_notch, &recenter, &open_full_item])
        .separator()
        .item(&idle_menu)
        .separator()
        .items(&[&floating, &live, &hide, &glow])
        .separator()
        .item(&auto)
        .separator()
        .item(&quit)
        .build()?;

    {
        let state = app.state::<AppState>();
        let mut map = state.tray_items.lock().unwrap();
        for it in [&hide_notch, &floating, &live, &hide, &glow, &auto] {
            map.insert(it.id().as_ref().to_string(), it.clone());
        }
        for it in &idle_items {
            map.insert(it.id().as_ref().to_string(), it.clone());
        }
    }

    TrayIconBuilder::with_id("tray")
        .icon(app.default_window_icon().cloned().expect("icon"))
        .tooltip("NeedleX")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, ev| {
            let id = ev.id().as_ref().to_string();
            let state = app.state::<AppState>();
            let item = state.tray_items.lock().unwrap().get(&id).cloned();
            match id.as_str() {
                "quit" => app.exit(0),
                "open_full" => show_full(app, false),
                "recenter" => {
                    place_island(app, 0.0);
                    update_setting(app, |s| s.notch_offset = 0.0);
                    if state.settings.lock().unwrap().notch_hidden {
                        apply_notch_hidden(app, false);
                    }
                }
                "hide_notch" => {
                    let hidden = !state.settings.lock().unwrap().notch_hidden;
                    apply_notch_hidden(app, hidden);
                }
                "autostart" => {
                    let al = app.autolaunch();
                    let on = !al.is_enabled().unwrap_or(false);
                    let _ = if on { al.enable() } else { al.disable() };
                    if let Some(i) = item {
                        let _ = i.set_checked(al.is_enabled().unwrap_or(on));
                    }
                }
                "floating" | "live" | "hide" | "glow" => {
                    let mut new_val = false;
                    update_setting(app, |s| {
                        let field = match id.as_str() {
                            "floating" => &mut s.floating,
                            "live" => &mut s.live_lyrics,
                            "hide" => &mut s.hide_idle,
                            _ => &mut s.glow,
                        };
                        *field = !*field;
                        new_val = *field;
                    });
                    if let Some(i) = item {
                        let _ = i.set_checked(new_val);
                    }
                }
                other if other.starts_with("idle_") => {
                    let m: u32 = other[5..].parse().unwrap_or(0);
                    update_setting(app, |s| s.auto_full_minutes = m);
                    let items = state.tray_items.lock().unwrap();
                    for c in IDLE_CHOICES {
                        if let Some(i) = items.get(&format!("idle_{c}")) {
                            let _ = i.set_checked(c == m);
                        }
                    }
                }
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}

fn create_full_window(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "full", WebviewUrl::App("full.html".into()))
        .title("NeedleX Now Playing")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .visible(false)
        .focused(false)
        .inner_size(800.0, 600.0)
        .build()?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .invoke_handler(tauri::generate_handler![
            media_control,
            media_seek,
            media_resend,
            set_hit_rect,
            get_settings,
            open_full,
            close_full,
            set_notch_hidden,
            move_notch,
            lyrics::fetch_lyrics,
            art::hires_art,
            art::capture_backdrop
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = load_settings(&handle);
            let (tx, rx) = mpsc::channel();
            app.manage(AppState {
                media_tx: Mutex::new(tx),
                hit: Mutex::new(hover::Rect::default()),
                lyrics_cache: Mutex::new(HashMap::new()),
                art_cache: Mutex::new(HashMap::new()),
                http: lyrics::client(),
                settings: Mutex::new(settings.clone()),
                playing: AtomicBool::new(false),
                geo_dirty: AtomicBool::new(true),
                dragging: AtomicBool::new(false),
                tray_items: Mutex::new(HashMap::new()),
            });
            position_window(&handle);
            if settings.notch_hidden {
                if let Some(main) = handle.get_webview_window("main") {
                    let _ = main.hide();
                }
            }
            create_full_window(&handle)?;
            build_tray(&handle, &settings)?;
            media::spawn(handle.clone(), rx);
            hover::spawn(handle.clone());
            audio::spawn(handle.clone());
            idle::spawn(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running NeedleX");
}
