//! Global input hooks, per-app profile switching and action dispatch.
//!
//! * WH_MOUSE_LL catches middle / back / forward.
//! * WH_KEYBOARD_LL catches F13..F24, which the side panel is set to send.
//! * EVENT_SYSTEM_FOREGROUND tells us which app is focused.
//!
//! Hook callbacks only look up the binding and hand a job to a worker thread,
//! so they always return quickly (Windows drops slow low-level hooks).

use crate::actions;
use crate::config::{Action, Config, DpiConfig};
use crate::device::DeviceCmd;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_F13, VK_F24};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub enum Job {
    Press(Action),
    Release(Action),
}

pub struct Engine {
    pub config: RwLock<Config>,
    pub active_profile: AtomicUsize,
    pub foreground: Mutex<String>,
    /// DPI currently in effect (profile override or global, plus DPI-button changes).
    pub dpi: Mutex<DpiConfig>,
    pub device: Sender<DeviceCmd>,
    jobs: Sender<Job>,
    held: Mutex<HashMap<&'static str, Action>>,
    /// Called when the active profile or DPI changes (UI refresh).
    notify: Box<dyn Fn() + Send + Sync>,
    own_exe: String,
}

static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();

pub fn engine() -> &'static Arc<Engine> {
    ENGINE.get().expect("engine not started")
}

pub fn try_engine() -> Option<&'static Arc<Engine>> {
    ENGINE.get()
}

pub fn start(config: Config, device: Sender<DeviceCmd>, notify: impl Fn() + Send + Sync + 'static) -> &'static Arc<Engine> {
    let (jobs, rx) = mpsc::channel::<Job>();
    let own_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    let dpi = config.dpi_for(0);
    let eng = Arc::new(Engine {
        config: RwLock::new(config),
        active_profile: AtomicUsize::new(0),
        foreground: Mutex::new(String::new()),
        dpi: Mutex::new(dpi),
        device,
        jobs,
        held: Mutex::new(HashMap::new()),
        notify: Box::new(notify),
        own_exe,
    });
    let _ = ENGINE.set(eng);

    std::thread::Builder::new().name("actions".into()).spawn(move || {
        for job in rx {
            run_job(job);
        }
    }).unwrap();

    std::thread::Builder::new().name("hooks".into()).spawn(hook_thread).unwrap();
    engine()
}

impl Engine {
    /// Replace the config (from the settings UI) and re-evaluate profile + DPI.
    pub fn set_config(&self, cfg: Config) {
        let polling = cfg.polling_rate;
        *self.config.write().unwrap() = cfg;
        let _ = self.device.send(DeviceCmd::PollingRate(polling));
        let fg = self.foreground.lock().unwrap().clone();
        self.select_profile(&fg, true);
    }

    fn select_profile(&self, exe: &str, force_dpi: bool) {
        let cfg = self.config.read().unwrap();
        let idx = cfg.profile_for_exe(exe);
        let old = self.active_profile.swap(idx, Ordering::SeqCst);
        if old != idx || force_dpi {
            let dpi = cfg.dpi_for(idx);
            let mut cur = self.dpi.lock().unwrap();
            if *cur != dpi || force_dpi {
                *cur = dpi.clone();
                let _ = self.device.send(DeviceCmd::ApplyDpi(dpi));
            }
        }
        drop(cfg);
        (self.notify)();
    }

    /// Step through the presets of the active profile (and remember the choice).
    fn step_dpi(&self, delta: isize) {
        let idx = self.active_profile.load(Ordering::SeqCst);
        let new = {
            let mut cur = self.dpi.lock().unwrap();
            let n = cur.presets.len() as isize;
            cur.active = ((cur.active as isize + delta).rem_euclid(n)) as usize;
            cur.clone()
        };
        let _ = self.device.send(DeviceCmd::ApplyDpi(new.clone()));
        {
            let mut cfg = self.config.write().unwrap();
            let in_profile = cfg.profiles.get(idx).and_then(|p| p.dpi.as_ref()).map_or(false, |d| d.presets == new.presets);
            if in_profile {
                cfg.profiles[idx].dpi.as_mut().unwrap().active = new.active;
            } else if cfg.dpi.presets == new.presets {
                cfg.dpi.active = new.active;
            }
            let _ = cfg.save();
        }
        (self.notify)();
    }
}

fn run_job(job: Job) {
    let e = engine();
    match job {
        Job::Press(action) => match action {
            Action::Default | Action::Disabled => {}
            Action::Keys { combo } => actions::keys_down(&actions::parse_combo(&combo)),
            Action::MouseButton { button } => actions::mouse_button(&button, false),
            Action::Media { key } => actions::tap_combo(&key),
            Action::DpiNext => e.step_dpi(1),
            Action::DpiPrev => e.step_dpi(-1),
            Action::DpiSet { dpi } => {
                let cfg = DpiConfig { presets: vec![crate::config::snap_dpi(dpi)], active: 0 };
                *e.dpi.lock().unwrap() = cfg.clone();
                let _ = e.device.send(DeviceCmd::ApplyDpi(cfg));
                (e.notify)();
            }
            Action::DpiHold { dpi } => {
                let _ = e.device.send(DeviceCmd::TempDpi(crate::config::snap_dpi(dpi)));
            }
            Action::Macro { steps } => {
                std::thread::spawn(move || actions::run_macro(&steps));
            }
            Action::Launch { path, args } => actions::launch(&path, &args),
            Action::Text { text } => {
                std::thread::spawn(move || actions::type_text(&text));
            }
        },
        Job::Release(action) => match action {
            Action::Keys { combo } => actions::keys_up(&actions::parse_combo(&combo)),
            Action::MouseButton { button } => actions::mouse_button(&button, true),
            Action::DpiHold { .. } => {
                let cur = e.dpi.lock().unwrap().clone();
                let _ = e.device.send(DeviceCmd::ApplyDpi(cur));
            }
            _ => {}
        },
    }
}

/// Decide what to do with a physical button event. Returns true to swallow it.
fn handle(trigger: &'static str, down: bool) -> bool {
    let Some(e) = ENGINE.get() else { return false };
    let mut held = e.held.lock().unwrap();
    if down {
        if let Some(a) = held.get(trigger) {
            // Key auto-repeat while held: keep swallowing, don't re-fire.
            return !a.is_default();
        }
        let profile = e.active_profile.load(Ordering::SeqCst);
        let action = e.config.read().unwrap().action_for(profile, trigger);
        let swallow = !action.is_default();
        held.insert(trigger, action.clone());
        if swallow {
            let _ = e.jobs.send(Job::Press(action));
        }
        swallow
    } else {
        match held.remove(trigger) {
            Some(a) if !a.is_default() => {
                let _ = e.jobs.send(Job::Release(a));
                true
            }
            Some(_) => false,
            None => {
                let profile = e.active_profile.load(Ordering::SeqCst);
                !e.config.read().unwrap().action_for(profile, trigger).is_default()
            }
        }
    }
}

const SIDE: [&str; 12] = [
    "side1", "side2", "side3", "side4", "side5", "side6", "side7", "side8", "side9", "side10", "side11", "side12",
];

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = &*(lparam as *const MSLLHOOKSTRUCT);
        if info.flags & LLMHF_INJECTED == 0 && info.dwExtraInfo != actions::MAGIC {
            let msg = wparam as u32;
            let hit = match msg {
                WM_MBUTTONDOWN => Some(("middle", true)),
                WM_MBUTTONUP => Some(("middle", false)),
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let down = msg == WM_XBUTTONDOWN;
                    match (info.mouseData >> 16) & 0xFFFF {
                        1 => Some(("back", down)),
                        2 => Some(("forward", down)),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some((trigger, down)) = hit {
                if handle(trigger, down) {
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = &*(lparam as *const KBDLLHOOKSTRUCT);
        let vk = info.vkCode as u16;
        if info.flags & LLKHF_INJECTED == 0 && (VK_F13..=VK_F24).contains(&vk) {
            let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            if handle(SIDE[(vk - VK_F13) as usize], down) {
                return 1;
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

unsafe extern "system" fn foreground_proc(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    on_foreground(hwnd);
}

fn on_foreground(hwnd: HWND) {
    let Some(e) = ENGINE.get() else { return };
    let Some(exe) = exe_of_window(hwnd) else { return };
    // Opening our own settings window shouldn't flip you back to the default profile.
    if exe == e.own_exe {
        return;
    }
    let changed = {
        let mut fg = e.foreground.lock().unwrap();
        let changed = *fg != exe;
        *fg = exe.clone();
        changed
    };
    if changed {
        e.select_profile(&exe, false);
    }
}

pub fn exe_of_window(hwnd: HWND) -> Option<String> {
    unsafe {
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(|s| s.to_lowercase())
    }
}

/// Executables that currently have a visible top-level window (for the profile app picker).
pub fn running_apps() -> Vec<String> {
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam as *mut Vec<String>);
        if IsWindowVisible(hwnd) != 0 && GetWindowTextLengthW(hwnd) > 0 && GetWindow(hwnd, GW_OWNER).is_null() {
            if let Some(exe) = exe_of_window(hwnd) {
                if !out.contains(&exe) {
                    out.push(exe);
                }
            }
        }
        1
    }
    let mut out: Vec<String> = Vec::new();
    unsafe {
        EnumWindows(Some(cb), &mut out as *mut _ as LPARAM);
    }
    out.sort();
    out
}

fn hook_thread() {
    unsafe {
        let hmod = GetModuleHandleW(std::ptr::null());
        let m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), hmod, 0);
        let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), hmod, 0);
        if m.is_null() || k.is_null() {
            eprintln!("failed to install input hooks");
        }
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(foreground_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        on_foreground(GetForegroundWindow());
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
