#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod config;
mod device;
mod input;

use config::{Config, DpiConfig};
use device::{DeviceCmd, DeviceStatus};
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt;

#[derive(Default)]
struct BatteryAlerts {
    /// Warning thresholds already notified since the last charge.
    fired: Vec<u8>,
    full_fired: bool,
}

struct AppState {
    device: Mutex<DeviceStatus>,
    alerts: Mutex<BatteryAlerts>,
    tray_battery: MenuItem<tauri::Wry>,
    tray_profile: MenuItem<tauri::Wry>,
}

#[derive(Serialize, Clone)]
struct Status {
    device: DeviceStatus,
    profile_index: usize,
    profile: String,
    foreground: String,
    dpi: DpiConfig,
}

fn build_status(app: &AppHandle) -> Status {
    let e = input::engine();
    let idx = e.active_profile.load(Ordering::SeqCst);
    let profile = e.config.read().unwrap().profiles.get(idx).map(|p| p.name.clone()).unwrap_or_default();
    Status {
        device: app.state::<AppState>().device.lock().unwrap().clone(),
        profile_index: idx,
        profile,
        foreground: e.foreground.lock().unwrap().clone(),
        dpi: e.dpi.lock().unwrap().clone(),
    }
}

fn battery_text(d: &DeviceStatus) -> String {
    match (d.connected, d.awake, d.battery, d.charging) {
        (false, ..) => "Mouse not connected".into(),
        (true, false, ..) => "Mouse asleep".into(),
        (_, _, Some(l), Some(true)) => format!("Battery {l}% (charging)"),
        (_, _, Some(l), _) => format!("Battery {l}%"),
        _ => "Battery unknown".into(),
    }
}

/// Push fresh status to the UI and the tray.
fn refresh(app: &AppHandle) {
    if input::try_engine().is_none() {
        return;
    }
    let status = build_status(app);
    let state = app.state::<AppState>();
    let _ = state.tray_battery.set_text(battery_text(&status.device));
    let _ = state.tray_profile.set_text(format!("Profile: {}", status.profile));
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(format!("MouseForge - {} - {}", battery_text(&status.device), status.profile)));
    }
    let _ = app.emit("status", status);
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        eprintln!("[notify] failed: {e}");
    }
}

/// Windows only shows toasts from a desktop app whose AppUserModelID has a
/// Start Menu shortcut (install.ps1 creates it). This registry entry adds the
/// display name and icon used on those toasts; it's rewritten on every launch.
fn register_toast_identity(identifier: &str) {
    use windows_sys::Win32::System::Registry::*;
    let icon_path = config::Config::path().with_file_name("icon.png");
    if let Some(dir) = icon_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&icon_path, include_bytes!("../icons/icon.png"));

    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let key_path = wide(&format!("Software\\Classes\\AppUserModelId\\{identifier}"));
    for (name, value) in [("DisplayName", "MouseForge".to_string()), ("IconUri", icon_path.display().to_string())] {
        let (n, v) = (wide(name), wide(&value));
        let rc = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, key_path.as_ptr(), n.as_ptr(), REG_SZ, v.as_ptr() as *const _, (v.len() * 2) as u32)
        };
        if rc != 0 {
            eprintln!("[notify] couldn't register {name}: {rc}");
        }
    }
}

fn on_device_status(app: &AppHandle, new: DeviceStatus) {
    let state = app.state::<AppState>();
    *state.device.lock().unwrap() = new.clone();

    if let (Some(level), Some(charging)) = (new.battery, new.charging) {
        let cfg = input::try_engine()
            .map(|e| e.config.read().unwrap().battery.clone())
            .unwrap_or_default();
        let mut alerts = state.alerts.lock().unwrap();
        if charging {
            alerts.fired.clear();
            if level >= 100 && cfg.notify_full && !alerts.full_fired {
                alerts.full_fired = true;
                notify(app, "Mouse fully charged", "Your Aerox 9 is at 100%. You can unplug it.");
            }
        } else {
            alerts.full_fired = false;
            // Forget thresholds we're back above (e.g. after a partial charge).
            alerts.fired.retain(|&w| level <= w);
            let mut crossed: Vec<u8> = cfg.warn_levels.iter().copied().filter(|&w| level <= w && !alerts.fired.contains(&w)).collect();
            if !crossed.is_empty() {
                crossed.sort();
                alerts.fired.extend(crossed.iter().copied());
                let urgent = level <= 10;
                notify(
                    app,
                    if urgent { "Mouse battery critical" } else { "Mouse battery low" },
                    &format!("Your Aerox 9 is at {level}%. Time to plug it in."),
                );
            }
        }
    }
    refresh(app);
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_config() -> Config {
    input::engine().config.read().unwrap().clone()
}

#[tauri::command]
fn save_config(app: AppHandle, config: Config) -> Result<Config, String> {
    let mut config = config;
    config.sanitize();
    eprintln!("[ui] save_config dpi={:?}", config.dpi);
    config.save().map_err(|e| format!("Couldn't save settings: {e}"))?;
    let e = input::engine();
    let _ = e.device.send(DeviceCmd::SetPollSecs(config.battery.poll_secs));
    e.set_config(config.clone());
    refresh(&app);
    Ok(config)
}

#[tauri::command]
fn get_status(app: AppHandle) -> Status {
    build_status(&app)
}

#[tauri::command]
fn running_apps() -> Vec<String> {
    input::running_apps()
}

#[tauri::command]
fn save_to_mouse() {
    let _ = input::engine().device.send(DeviceCmd::SaveToMouse);
}

/// mode: "fkeys" (F13-F24, needed for remapping) or "numbers" (factory default).
#[tauri::command]
async fn set_side_keys(mode: String) -> Result<(), String> {
    let mode = match mode.as_str() {
        "fkeys" => device::SideKeys::FKeys,
        "numbers" => device::SideKeys::Numbers,
        _ => return Err("unknown mode".into()),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    input::engine().device.send(DeviceCmd::SetSideKeys(mode, tx)).map_err(|e| e.to_string())?;
    rx.recv_timeout(std::time::Duration::from_secs(8)).map_err(|_| "The mouse didn't respond".to_string())?
}

#[tauri::command]
fn refresh_battery() {
    let _ = input::engine().device.send(DeviceCmd::RefreshBattery);
}

#[tauri::command]
fn test_notification(app: AppHandle) {
    let d = app.state::<AppState>().device.lock().unwrap().clone();
    notify(&app, "MouseForge test", &format!("Notifications are working. {}.", battery_text(&d)));
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(|e| e.to_string())
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_status,
            running_apps,
            save_to_mouse,
            set_side_keys,
            refresh_battery,
            test_notification,
            get_autostart,
            set_autostart
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            register_toast_identity(&app.config().identifier);

            // Tray
            let tray_battery = MenuItem::with_id(app, "battery", "Battery ...", false, None::<&str>)?;
            let tray_profile = MenuItem::with_id(app, "profile", "Profile: Default", false, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Open MouseForge", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&tray_battery, &tray_profile, &sep, &open, &quit])?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("MouseForge")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            app.manage(AppState {
                device: Mutex::new(DeviceStatus::default()),
                alerts: Mutex::new(BatteryAlerts::default()),
                tray_battery,
                tray_profile,
            });

            // Device + input engine
            let cfg = Config::load();
            let h = handle.clone();
            let device = device::spawn(cfg.battery.poll_secs, cfg.dpi_for(0), cfg.polling_rate, move |s| on_device_status(&h, s));
            let h = handle.clone();
            input::start(cfg, device, move || refresh(&h));

            // Closing the window just hides it to the tray.
            if let Some(w) = app.get_webview_window("main") {
                let w2 = w.clone();
                w.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w2.hide();
                    }
                });
            }
            if !std::env::args().any(|a| a == "--minimized") {
                show_main(&handle);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running MouseForge");
}
