//! User configuration: DPI presets, per-app profiles and button bindings.
//! Stored as JSON in %APPDATA%\MouseForge\config.json.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MacroStep {
    /// Press and release a combo, e.g. "Ctrl+C".
    Tap { combo: String },
    /// Hold a single key down.
    Down { key: String },
    /// Release a single key.
    Up { key: String },
    /// Wait.
    Delay { ms: u64 },
    /// Type literal text.
    Text { text: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Let the button do whatever it normally does.
    #[default]
    Default,
    /// Swallow the button completely.
    Disabled,
    /// Hold a key combo for as long as the button is held, e.g. "Ctrl+Shift+T".
    Keys { combo: String },
    /// left | right | middle | back | forward
    MouseButton { button: String },
    /// play_pause | next | prev | stop | volume_up | volume_down | mute
    Media { key: String },
    /// Move to the next DPI preset (wraps around).
    DpiNext,
    /// Move to the previous DPI preset (wraps around).
    DpiPrev,
    /// Jump to a fixed DPI.
    DpiSet { dpi: u32 },
    /// "Sniper" button: use this DPI only while held.
    DpiHold { dpi: u32 },
    /// Run a sequence of steps.
    Macro { steps: Vec<MacroStep> },
    /// Open a program, file or URL.
    Launch {
        path: String,
        #[serde(default)]
        args: String,
    },
    /// Type a block of text.
    Text { text: String },
}

impl Action {
    pub fn is_default(&self) -> bool {
        matches!(self, Action::Default)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DpiConfig {
    /// 1 to 5 presets, 100..=18000 in steps of 100.
    pub presets: Vec<u32>,
    /// Index of the active preset.
    pub active: usize,
}

impl Default for DpiConfig {
    fn default() -> Self {
        Self { presets: vec![400, 800, 1200, 2400, 3200], active: 1 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Profile {
    pub name: String,
    /// Executable names this profile applies to, e.g. "cs2.exe". Empty for the default profile.
    #[serde(default)]
    pub apps: Vec<String>,
    /// trigger name -> action. Missing triggers fall back to the default profile.
    #[serde(default)]
    pub bindings: HashMap<String, Action>,
    /// Optional DPI override while this profile is active.
    #[serde(default)]
    pub dpi: Option<DpiConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BatteryConfig {
    /// Notify when the battery drops to or below each of these levels.
    pub warn_levels: Vec<u8>,
    /// Notify once charging reaches 100%.
    pub notify_full: bool,
    /// How often to ask the mouse for its battery level.
    pub poll_secs: u64,
}

impl Default for BatteryConfig {
    fn default() -> Self {
        Self { warn_levels: vec![20, 10, 5], notify_full: true, poll_secs: 60 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub dpi: DpiConfig,
    #[serde(default = "default_polling")]
    pub polling_rate: u32,
    /// profiles[0] is always the default profile.
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub battery: BatteryConfig,
}

fn default_polling() -> u32 {
    1000
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dpi: DpiConfig::default(),
            polling_rate: 1000,
            profiles: vec![Profile {
                name: "Default".into(),
                apps: vec![],
                bindings: HashMap::new(),
                dpi: None,
            }],
            battery: BatteryConfig::default(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        base.join("MouseForge").join("config.json")
    }

    pub fn load() -> Self {
        let mut cfg: Config = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        cfg.sanitize();
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap())?;
        std::fs::rename(tmp, path)
    }

    /// Clamp everything into ranges the hardware accepts.
    pub fn sanitize(&mut self) {
        if self.profiles.is_empty() {
            self.profiles = Config::default().profiles;
        }
        self.profiles[0].apps.clear();
        sanitize_dpi(&mut self.dpi);
        for p in &mut self.profiles {
            if let Some(d) = &mut p.dpi {
                sanitize_dpi(d);
            }
            for a in &mut p.apps {
                *a = a.trim().to_lowercase();
            }
            p.apps.retain(|a| !a.is_empty());
        }
        if ![125, 250, 500, 1000].contains(&self.polling_rate) {
            self.polling_rate = 1000;
        }
        self.battery.poll_secs = self.battery.poll_secs.clamp(15, 3600);
    }

    /// Profile index for a foreground executable name (lowercase).
    pub fn profile_for_exe(&self, exe: &str) -> usize {
        self.profiles
            .iter()
            .position(|p| p.apps.iter().any(|a| a == exe))
            .unwrap_or(0)
    }

    /// Action bound to `trigger` in `profile`, falling back to the default profile.
    pub fn action_for(&self, profile: usize, trigger: &str) -> Action {
        let lookup = |i: usize| self.profiles.get(i).and_then(|p| p.bindings.get(trigger)).cloned();
        match lookup(profile) {
            Some(a) if !a.is_default() => a,
            _ => lookup(0).unwrap_or_default(),
        }
    }

    /// DPI settings that should be live while `profile` is active.
    pub fn dpi_for(&self, profile: usize) -> DpiConfig {
        self.profiles.get(profile).and_then(|p| p.dpi.clone()).unwrap_or_else(|| self.dpi.clone())
    }
}

pub fn snap_dpi(dpi: u32) -> u32 {
    ((dpi.clamp(100, 18000) + 50) / 100) * 100
}

fn sanitize_dpi(d: &mut DpiConfig) {
    d.presets.truncate(5);
    if d.presets.is_empty() {
        d.presets = DpiConfig::default().presets;
    }
    for v in &mut d.presets {
        *v = snap_dpi(*v);
    }
    if d.active >= d.presets.len() {
        d.active = 0;
    }
}
