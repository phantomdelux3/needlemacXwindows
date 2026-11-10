#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod art;
mod audio;
mod display;
mod hover;
mod idle;
mod lyrics;
mod media;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use media::MediaCmd;

/// Auto full-screen choices offered in the tray (minutes, 0 = off).
const IDLE_CHOICES: [u32; 6] = [0, 1, 3, 5, 10, 15];
/// Glitter colour choices offered in the tray.
const GLITTER_CHOICES: [(&str, &str); 4] = [
    ("off", "Off"),
    ("white", "White"),
    ("multi", "Multicolour"),
    ("album", "Album colours"),
];

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
    /// Monitor (system name) the notch lives on; empty = main display.
    pub monitor: String,
    /// Glitter in the full-screen view: "off" | "white" | "multi" | "album".
    pub glitter: String,
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
            monitor: String::new(),
            glitter: "off".into(),
        }
    }
}

pub struct AppState {
    pub media_tx: Mutex<Sender<MediaCmd>>,
    pub hit: Mutex<hover::Rect>,
    pub lyrics_cache: Mutex<HashMap<String, lyrics::LyricsResult>>,
    pub art_cache: Mutex<HashMap<String, Option<String>>>,
    /// one cover lookup at a time (both windows ask for the same song)
    pub art_lock: tokio::sync::Mutex<()>,
    pub http: reqwest::Client,
    pub settings: Mutex<Settings>,
    /// Something is playing right now (set by the media watcher).
    pub playing: AtomicBool,
    /// The island window moved – hover detection must re-read its position.
    pub geo_dirty: AtomicBool,
    /// The notch is being dragged – keep the window interactive.
    pub dragging: AtomicBool,
    /// Zoom applied to the island so it fits small / heavily-scaled screens.
    pub zoom: Mutex<f64>,
    /// System names of the monitors listed in the tray, in menu order.
    pub monitors: Mutex<Vec<String>>,
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

pub fn update_setting(app: &AppHandle, f: impl FnOnce(&mut Settings)) {
    let state = app.state::<AppState>();
    let s = {
        let mut s = state.settings.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    save_settings(app, &s);
    let _ = app.emit("settings", s);
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

/// Start dragging the notch sideways (tracked natively until the button is released).
#[tauri::command]
fn drag_notch_start(app: AppHandle) {
    display::drag(app);
}

// ---------- windows ----------

/// Opens the full-screen view on the notch's monitor. `auto` = idle timer.
pub fn show_full(app: &AppHandle, auto: bool) {
    let Some(full) = app.get_webview_window("full") else { return };
    display::fit_full(app);
    if let Some(main) = app.get_webview_window("main") {
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

// ---------- tray ----------

fn idle_label(m: u32) -> String {
    match m {
        0 => "Off".into(),
        1 => "After 1 minute".into(),
        m => format!("After {m} minutes"),
    }
}

/// Builds the tray menu from the current settings and display setup.
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let s = app.state::<AppState>().settings.lock().unwrap().clone();
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let check = |id: &str, label: &str, on: bool| CheckMenuItemBuilder::with_id(id, label).checked(on).build(app);

    let hide_notch = check("hide_notch", "Hide notch", s.notch_hidden)?;
    let floating = check("floating", "Floating island (pill shape)", s.floating)?;
    let live = check("live", "Live lyrics under the island", s.live_lyrics)?;
    let hide = check("hide", "Hide when nothing is playing", s.hide_idle)?;
    let glow = check("glow", "Beat glow", s.glow)?;
    let auto = check("autostart", "Launch at startup", autostart_on)?;

    let mut items: Vec<CheckMenuItem<Wry>> = vec![
        hide_notch.clone(), floating.clone(), live.clone(), hide.clone(), glow.clone(), auto.clone(),
    ];

    let mut idle_menu = SubmenuBuilder::new(app, "Full screen when idle");
    for m in IDLE_CHOICES {
        let it = check(&format!("idle_{m}"), &idle_label(m), s.auto_full_minutes == m)?;
        idle_menu = idle_menu.item(&it);
        items.push(it);
    }
    let idle_menu = idle_menu.build()?;

    // which display the notch lives on
    let mons = display::monitors(app);
    let prim = app.primary_monitor().ok().flatten().and_then(|m| m.name().cloned()).unwrap_or_default();
    let names: Vec<String> = mons.iter().map(|m| m.name().cloned().unwrap_or_default()).collect();
    let chosen_exists = names.iter().any(|n| *n == s.monitor);
    let mut disp_menu = SubmenuBuilder::new(app, "Show notch on");
    let main_item = check("mon_main", "Main display", s.monitor.is_empty() || !chosen_exists)?;
    disp_menu = disp_menu.item(&main_item).separator();
    items.push(main_item);
    for (i, m) in mons.iter().enumerate() {
        let name = &names[i];
        let it = check(&format!("mon_{i}"), &display::monitor_label(i, m, *name == prim), chosen_exists && *name == s.monitor)?;
        disp_menu = disp_menu.item(&it);
        items.push(it);
    }
    let disp_menu = disp_menu.build()?;

    // glitter in the full-screen view
    let mut glit_menu = SubmenuBuilder::new(app, "Glitter (full screen)");
    for (id, label) in GLITTER_CHOICES {
        let it = check(&format!("glitter_{id}"), label, s.glitter == id)?;
        glit_menu = glit_menu.item(&it);
        items.push(it);
    }
    let glit_menu = glit_menu.build()?;

    let open_full_item = MenuItemBuilder::with_id("open_full", "Open full screen").build(app)?;
    let recenter = MenuItemBuilder::with_id("recenter", "Re-center notch").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit NeedleX").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[&hide_notch, &recenter, &open_full_item])
        .item(&disp_menu)
        .separator()
        .item(&idle_menu)
        .item(&glit_menu)
        .separator()
        .items(&[&floating, &live, &hide, &glow])
        .separator()
        .item(&auto)
        .separator()
        .item(&quit)
        .build()?;

    let state = app.state::<AppState>();
    *state.monitors.lock().unwrap() = names;
    let mut map = state.tray_items.lock().unwrap();
    map.clear();
    for it in items {
        map.insert(it.id().as_ref().to_string(), it);
    }
    Ok(menu)
}

/// Rebuilds the tray menu (e.g. after monitors were plugged in / removed).
pub fn rebuild_menu(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("tray"), build_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    let item = state.tray_items.lock().unwrap().get(id).cloned();
    match id {
        "quit" => app.exit(0),
        "open_full" => show_full(app, false),
        "recenter" => {
            update_setting(app, |s| s.notch_offset = 0.0);
            display::place_island(app, 0.0);
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
                let field = match id {
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
        other if other.starts_with("glitter_") => {
            let g = other["glitter_".len()..].to_string();
            update_setting(app, |s| s.glitter = g.clone());
            let items = state.tray_items.lock().unwrap();
            for (id, _) in GLITTER_CHOICES {
                if let Some(i) = items.get(&format!("glitter_{id}")) {
                    let _ = i.set_checked(id == g);
                }
            }
        }
        other if other.starts_with("mon_") => {
            let name = if other == "mon_main" {
                String::new()
            } else {
                let i: usize = other[4..].parse().unwrap_or(usize::MAX);
                state.monitors.lock().unwrap().get(i).cloned().unwrap_or_default()
            };
            // a new screen starts centred
            update_setting(app, |s| {
                s.monitor = name;
                s.notch_offset = 0.0;
            });
            let a = app.clone();
            std::thread::spawn(move || {
                display::relayout(&a);
                rebuild_menu(&a);
            });
        }
        _ => {}
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    TrayIconBuilder::with_id("tray")
        .icon(app.default_window_icon().cloned().expect("icon"))
        .tooltip("NeedleX")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()))
        .build(app)?;
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
            drag_notch_start,
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
                art_lock: tokio::sync::Mutex::new(()),
                http: lyrics::client(),
                settings: Mutex::new(settings.clone()),
                playing: AtomicBool::new(false),
                geo_dirty: AtomicBool::new(true),
                dragging: AtomicBool::new(false),
                zoom: Mutex::new(1.0),
                monitors: Mutex::new(Vec::new()),
                tray_items: Mutex::new(HashMap::new()),
            });
            if settings.notch_hidden {
                if let Some(main) = handle.get_webview_window("main") {
                    let _ = main.hide();
                }
            }
            create_full_window(&handle)?;
            build_tray(&handle)?;
            display::spawn(handle.clone()); // first layout + watches display changes
            media::spawn(handle.clone(), rx);
            hover::spawn(handle.clone());
            audio::spawn(handle.clone());
            idle::spawn(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running NeedleX");
}
