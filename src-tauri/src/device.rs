//! HID protocol for the SteelSeries Aerox 9 Wireless.
//!
//! Command bytes come from rivalcfg (https://github.com/flozz/rivalcfg, WTFPL):
//! every command is an output report on interface 3; in 2.4 GHz mode the
//! command byte is OR-ed with 0x40 and the mouse echoes it back as the first
//! byte of its reply.

use crate::config::{DpiConfig, IdleLighting, LightMode, LightingConfig, PowerConfig};
use crate::lighting::{self, Frame};
use hidapi::{HidApi, HidDevice};
use serde::Serialize;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

const VID: u16 = 0x1038;
const IFACE: i32 = 3;
const WIRELESS_FLAG: u8 = 0x40;

/// (product id, is 2.4 GHz wireless, name)
const MODELS: &[(u16, bool, &str)] = &[
    (0x1858, true, "Aerox 9 Wireless (2.4 GHz)"),
    (0x1874, true, "Aerox 9 Wireless WOW (2.4 GHz)"),
    (0x185A, false, "Aerox 9 Wireless (USB cable)"),
    (0x1876, false, "Aerox 9 Wireless WOW (USB cable)"),
];

const CMD_SAVE: u8 = 0x11;
const CMD_POLLING: u8 = 0x2B;
const CMD_DPI: u8 = 0x2D;
const CMD_BATTERY: u8 = 0x92;

/// TrueMove Air sensor CPI table (dpi, register value).
pub const DPI_TABLE: &[(u32, u8)] = &[(100,0x00),(200,0x02),(300,0x03),(400,0x04),(500,0x05),(600,0x06),(700,0x07),(800,0x09),(900,0x0A),(1000,0x0B),(1100,0x0C),(1200,0x0D),(1300,0x0E),(1400,0x10),(1500,0x11),(1600,0x12),(1700,0x13),(1800,0x14),(1900,0x16),(2000,0x17),(2100,0x18),(2200,0x19),(2300,0x1A),(2400,0x1B),(2500,0x1D),(2600,0x1E),(2700,0x1F),(2800,0x20),(2900,0x21),(3000,0x23),(3100,0x25),(3200,0x26),(3300,0x27),(3400,0x28),(3500,0x29),(3600,0x2A),(3700,0x2C),(3800,0x2D),(3900,0x2E),(4000,0x2F),(4100,0x30),(4200,0x32),(4300,0x33),(4400,0x34),(4500,0x35),(4600,0x36),(4700,0x38),(4800,0x39),(4900,0x3A),(5000,0x3B),(5100,0x3C),(5200,0x3E),(5300,0x3F),(5400,0x40),(5500,0x41),(5600,0x42),(5700,0x44),(5800,0x45),(5900,0x46),(6000,0x47),(6100,0x48),(6200,0x4A),(6300,0x4B),(6400,0x4C),(6500,0x4D),(6600,0x4E),(6700,0x4F),(6800,0x51),(6900,0x52),(7000,0x53),(7100,0x54),(7200,0x55),(7300,0x57),(7400,0x58),(7500,0x59),(7600,0x5A),(7700,0x5B),(7800,0x5D),(7900,0x5E),(8000,0x5F),(8100,0x60),(8200,0x61),(8300,0x62),(8400,0x64),(8500,0x65),(8600,0x66),(8700,0x67),(8800,0x68),(8900,0x6A),(9000,0x6B),(9100,0x6C),(9200,0x6D),(9300,0x6E),(9400,0x70),(9500,0x71),(9600,0x72),(9700,0x73),(9800,0x74),(9900,0x76),(10000,0x77),(10100,0x78),(10200,0x79),(10300,0x7A),(10400,0x7B),(10500,0x7D),(10600,0x7E),(10700,0x7F),(10800,0x80),(10900,0x81),(11000,0x83),(11100,0x84),(11200,0x85),(11300,0x86),(11400,0x87),(11500,0x89),(11600,0x8A),(11700,0x8B),(11800,0x8C),(11900,0x8D),(12000,0x8F),(12100,0x90),(12200,0x91),(12300,0x92),(12400,0x93),(12500,0x95),(12600,0x96),(12700,0x97),(12800,0x98),(12900,0x99),(13000,0x9A),(13100,0x9C),(13200,0x9D),(13300,0x9E),(13400,0x9F),(13500,0xA0),(13600,0xA2),(13700,0xA3),(13800,0xA4),(13900,0xA5),(14000,0xA6),(14100,0xA8),(14200,0xA9),(14300,0xAA),(14400,0xAB),(14500,0xAC),(14600,0xAE),(14700,0xAF),(14800,0xB0),(14900,0xB1),(15000,0xB2),(15100,0xB4),(15200,0xB5),(15300,0xB6),(15400,0xB7),(15500,0xB8),(15600,0xB9),(15700,0xBB),(15800,0xBC),(15900,0xBD),(16000,0xBE),(16100,0xBF),(16200,0xC1),(16300,0xC2),(16400,0xC3),(16500,0xC4),(16600,0xC5),(16700,0xC7),(16800,0xC8),(16900,0xC9),(17000,0xCA),(17100,0xCB),(17200,0xCC),(17300,0xCD),(17400,0xCF),(17500,0xD0),(17600,0xD1),(17700,0xD2),(17800,0xD3),(17900,0xD5),(18000,0xD6)];

fn dpi_code(dpi: u32) -> u8 {
    let dpi = crate::config::snap_dpi(dpi);
    DPI_TABLE.iter().find(|(d, _)| *d == dpi).map(|(_, c)| *c).unwrap_or(0x12)
}

// ---- Button table (reverse-engineered from SteelSeries GG with USBPcap) ----
//
// Command 0x2A, sent as 3 pages: [page, slot, slot, ...], 5 bytes per slot,
// 6 + 6 + 8 = 20 slots, followed by the save command. A slot is [type, code, 0, 0, 0];
// type 0x51 = keyboard key with HID usage `code`.
const CMD_BUTTONS: u8 = 0x2A;
const PAGE_SLOTS: [usize; 3] = [6, 6, 8];
/// Physical side button number (1..12) held by each of table slots 6..18.
const SIDE_SLOT_ORDER: [u8; 12] = [3, 6, 9, 12, 2, 5, 8, 11, 1, 4, 7, 10];
const KEY: u8 = 0x51;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SideKeys {
    /// Side buttons 1..12 send F13..F24 (what MouseForge listens for).
    FKeys,
    /// Factory default: 1 2 3 4 5 6 7 8 9 0 - =
    Numbers,
}

fn side_usage(mode: SideKeys, button: u8) -> u8 {
    match mode {
        SideKeys::FKeys => 0x68 + (button - 1), // HID F13 = 0x68 .. F24 = 0x73
        SideKeys::Numbers => match button {
            1..=9 => 0x1E + (button - 1),
            10 => 0x27,
            11 => 0x2D,
            _ => 0x2E,
        },
    }
}

/// Full 20-slot table. Every slot except the side panel is exactly what GG
/// writes (left, right, wheel click, scroll and DPI entries), so the main
/// buttons are never changed.
fn button_table(mode: SideKeys) -> Vec<[u8; 5]> {
    let mut t = vec![[0x01, 0, 0, 0, 0], [0x02, 0, 0, 0, 0], [0x03, 0, 0, 0, 0], [0x33, 0, 0, 0, 0], [0x34, 0, 0, 0, 0], [0x06, 0, 0, 0, 0]];
    for &b in &SIDE_SLOT_ORDER {
        t.push([KEY, side_usage(mode, b), 0, 0, 0]);
    }
    t.push([0x31, 0, 0, 0, 0]);
    t.push([0x32, 0, 0, 0, 0]);
    t
}

pub enum DeviceCmd {
    /// Rewrite the side panel keys and save them to the mouse.
    SetSideKeys(SideKeys, Sender<Result<(), String>>),
    /// Write a full preset list + active preset.
    ApplyDpi(DpiConfig),
    /// Write a single temporary DPI (sniper hold); ApplyDpi restores.
    TempDpi(u32),
    PollingRate(u32),
    SaveToMouse,
    RefreshBattery,
    SetPollSecs(u64),
    /// New lighting settings (effect, colours, idle lighting).
    SetLighting(LightingConfig),
    /// Sleep and dim timers; written and saved to the mouse.
    SetPower(PowerConfig),
}

const CMD_LIGHT: u8 = 0x21;
const CMD_ONBOARD_RAINBOW: u8 = 0x22;
const CMD_DIM_TIMER: u8 = 0x23;
const CMD_REACTIVE: u8 = 0x26;
const CMD_IDLE_LIGHTING: u8 = 0x27;
const CMD_SLEEP_TIMER: u8 = 0x29;
/// ~25 frames per second for animated effects.
const FRAME_INTERVAL: Duration = Duration::from_millis(40);

fn le24(v: u32) -> [u8; 3] {
    let b = v.to_le_bytes();
    [b[0], b[1], b[2]]
}

fn write_frame(d: &Open, f: &Frame) -> hidapi::HidResult<Option<[u8; 64]>> {
    let mut p = vec![3u8];
    for (zone, rgb) in f.iter().enumerate() {
        p.push(zone as u8);
        p.extend_from_slice(rgb);
    }
    d.send(CMD_LIGHT, &p)
}

fn write_power(d: &Open, p: &PowerConfig) -> hidapi::HidResult<()> {
    d.send(CMD_SLEEP_TIMER, &le24(p.sleep_minutes * 60_000))?;
    let mut dim = vec![0x0F, 0x01, 0x01, 0x00];
    dim.extend_from_slice(&le24(p.dim_seconds * 1000));
    d.send(CMD_DIM_TIMER, &dim)?;
    d.send(CMD_SAVE, &[0x00])?;
    Ok(())
}

fn write_idle_lighting(d: &Open, idle: IdleLighting) -> hidapi::HidResult<()> {
    let v: [u8; 2] = match idle {
        IdleLighting::Rainbow => [0x01, 0x00],
        IdleLighting::Off => [0x00, 0x00],
    };
    d.send(CMD_IDLE_LIGHTING, &v)?;
    d.send(CMD_SAVE, &[0x00])?;
    Ok(())
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct DeviceStatus {
    /// Receiver / cable found.
    pub connected: bool,
    /// Mouse answered (awake and linked).
    pub awake: bool,
    pub model: Option<String>,
    pub battery: Option<u8>,
    pub charging: Option<bool>,
    /// DPI we last wrote to the sensor.
    pub live_dpi: Option<u32>,
}

struct Open {
    dev: HidDevice,
    flag: u8,
    model: &'static str,
}

impl Open {
    fn write(&self, cmd: u8, payload: &[u8]) -> hidapi::HidResult<()> {
        let mut buf = Vec::with_capacity(payload.len() + 2);
        buf.push(0x00); // report id
        buf.push(cmd | self.flag);
        buf.extend_from_slice(payload);
        self.dev.write(&buf).map(|_| ())
    }

    /// Send a command and wait for the mouse's acknowledgement (the 2.4 GHz
    /// receiver expects its replies to be read).
    fn send(&self, cmd: u8, payload: &[u8]) -> hidapi::HidResult<Option<[u8; 64]>> {
        self.exchange(cmd, payload)
    }

    /// Send `cmd` and wait for the reply that echoes it. Other traffic on the
    /// interface (e.g. SteelSeries GG still running) is skipped.
    fn query(&self, cmd: u8) -> hidapi::HidResult<Option<[u8; 64]>> {
        self.exchange(cmd, &[])
    }

    fn exchange(&self, cmd: u8, payload: &[u8]) -> hidapi::HidResult<Option<[u8; 64]>> {
        let mut buf = [0u8; 64];
        while self.dev.read_timeout(&mut buf, 5)? > 0 {}
        self.write(cmd, payload)?;
        let deadline = Instant::now() + Duration::from_millis(600);
        while Instant::now() < deadline {
            let n = self.dev.read_timeout(&mut buf, 100)?;
            if n > 0 && buf[0] == cmd | self.flag {
                return Ok(Some(buf));
            }
        }
        Ok(None)
    }
}

fn open(api: &mut HidApi) -> Option<Open> {
    let _ = api.refresh_devices();
    for &(pid, wireless, model) in MODELS {
        let info = api
            .device_list()
            .find(|d| d.vendor_id() == VID && d.product_id() == pid && d.interface_number() == IFACE);
        if let Some(info) = info {
            if let Ok(dev) = info.open_device(api) {
                return Some(Open { dev, flag: if wireless { WIRELESS_FLAG } else { 0 }, model });
            }
        }
    }
    None
}

/// Start the device thread. `on_status` is called whenever the status changes.
pub fn spawn(
    poll_secs: u64,
    initial: DpiConfig,
    polling_rate: u32,
    lighting: LightingConfig,
    on_status: impl Fn(DeviceStatus) + Send + 'static,
) -> Sender<DeviceCmd> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("device".into())
        .spawn(move || run(rx, poll_secs, initial, polling_rate, lighting, on_status))
        .expect("spawn device thread");
    tx
}

fn run(
    rx: Receiver<DeviceCmd>,
    mut poll_secs: u64,
    mut dpi: DpiConfig,
    mut polling_rate: u32,
    mut light: LightingConfig,
    on_status: impl Fn(DeviceStatus),
) {
    let t0 = Instant::now();
    let mut light_dirty = true;
    let mut next_frame = Instant::now();
    let mut api = match HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hidapi init failed: {e}");
            return;
        }
    };
    let mut dev: Option<Open> = None;
    let mut status = DeviceStatus::default();
    let mut next_poll = Instant::now();
    let mut temp_dpi: Option<u32> = None;

    loop {
        // (Re)connect.
        if dev.is_none() {
            dev = open(&mut api);
            if dev.is_some() {
                next_poll = Instant::now();
            }
        }

        let streaming = dev.is_some() && status.awake && lighting::animated(&light);
        let mut wait = next_poll.saturating_duration_since(Instant::now());
        if streaming {
            wait = wait.min(next_frame.saturating_duration_since(Instant::now()));
        }
        let cmd = match rx.recv_timeout(wait) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };

        let mut new = status.clone();
        let mut push_dpi = false;
        match cmd {
            Some(DeviceCmd::ApplyDpi(d)) => {
                dpi = d;
                temp_dpi = None;
                push_dpi = true;
            }
            Some(DeviceCmd::TempDpi(v)) => {
                temp_dpi = Some(v);
                push_dpi = true;
            }
            Some(DeviceCmd::PollingRate(hz)) => {
                // Only touch the polling rate when it really changes: the
                // firmware re-initialises the sensor afterwards, so re-send DPI.
                if hz != polling_rate {
                    polling_rate = hz;
                    if let Some(d) = &dev {
                        let res = d.send(CMD_POLLING, &[polling_code(hz)]);
                        eprintln!("[device] polling {hz} -> {:02x?}", res.as_ref().map(|r| r.map(|b| [b[0], b[1]])));
                        std::thread::sleep(Duration::from_millis(150));
                        push_dpi = true;
                    }
                }
            }
            Some(DeviceCmd::SetSideKeys(mode, reply)) => {
                let result = match &dev {
                    None => Err("Mouse receiver not found".to_string()),
                    Some(_) if !status.awake => Err("The mouse is asleep. Move it and try again.".to_string()),
                    Some(d) => write_buttons(d, mode),
                };
                eprintln!("[device] side keys {mode:?} -> {result:?}");
                let _ = reply.send(result);
            }
            Some(DeviceCmd::SaveToMouse) => {
                if let Some(d) = &dev {
                    let _ = d.send(CMD_SAVE, &[0x00]);
                }
            }
            Some(DeviceCmd::SetLighting(l)) => {
                if let Some(d) = &dev {
                    if l.idle != light.idle {
                        let res = write_idle_lighting(d, l.idle);
                        eprintln!("[device] idle lighting {:?} -> {res:?}", l.idle);
                    }
                    // The click flash is done by the mouse; save it so it also
                    // applies when MouseForge isn't running.
                    if l.reactive != light.reactive {
                        let res = d.send(CMD_REACTIVE, &lighting::reactive_payload(&l)).and_then(|_| d.send(CMD_SAVE, &[0x00]));
                        eprintln!("[device] reactive {:?} -> {:?}", l.reactive, res.map(|r| r.is_some()));
                    }
                }
                light = l;
                light_dirty = true;
            }
            Some(DeviceCmd::SetPower(p)) => {
                if let Some(d) = &dev {
                    let res = write_power(d, &p);
                    eprintln!("[device] power {p:?} -> {res:?}");
                }
            }
            Some(DeviceCmd::RefreshBattery) => next_poll = Instant::now(),
            Some(DeviceCmd::SetPollSecs(s)) => {
                poll_secs = s;
                next_poll = Instant::now();
            }
            None => {}
        }

        // Periodic battery poll (also our "is the mouse there" check).
        if Instant::now() >= next_poll {
            next_poll = Instant::now() + Duration::from_secs(poll_secs);
            match &dev {
                None => new = DeviceStatus::default(),
                Some(d) => match d.query(CMD_BATTERY) {
                    Err(_) => {
                        dev = None;
                        new = DeviceStatus::default();
                        // Try reconnecting soon instead of waiting a full interval.
                        next_poll = Instant::now() + Duration::from_secs(3);
                    }
                    Ok(None) => {
                        new.connected = true;
                        new.awake = false;
                        new.model = Some(d.model.into());
                        new.battery = None;
                        new.charging = None;
                        next_poll = Instant::now() + Duration::from_secs(poll_secs.min(10));
                    }
                    Ok(Some(buf)) => {
                        let raw = buf[1];
                        let charging = raw & 0x80 != 0;
                        let level = ((raw & 0x7F).saturating_sub(1) as u32 * 5).min(100) as u8;
                        let woke = !status.awake;
                        new.connected = true;
                        new.awake = true;
                        new.model = Some(d.model.into());
                        new.battery = Some(level);
                        new.charging = Some(charging);
                        if woke {
                            // Mouse just (re)appeared: it may have reverted to its
                            // onboard settings, so push ours again.
                            // Polling rate first: it re-initialises the sensor,
                            // which would wipe a DPI written before it.
                            let _ = d.send(CMD_POLLING, &[polling_code(polling_rate)]);
                            std::thread::sleep(Duration::from_millis(150));
                            push_dpi = true;
                            light_dirty = true;
                        }
                    }
                },
            }
        }

        if push_dpi {
            if let Some(d) = &dev {
                let (presets, active) = match temp_dpi {
                    Some(v) => (vec![v], 0),
                    None => (dpi.presets.clone(), dpi.active),
                };
                let mut payload = vec![presets.len() as u8, active as u8];
                payload.extend(presets.iter().map(|&p| dpi_code(p)));
                let res = d.send(CMD_DPI, &payload);
                eprintln!("[device] dpi {presets:?} active={active} -> {:02x?}", res.as_ref().map(|r| r.map(|b| [b[0], b[1], b[2]])));
                if res.is_ok() {
                    new.live_dpi = Some(presets[active.min(presets.len() - 1)]);
                } else {
                    dev = None;
                    new = DeviceStatus::default();
                }
            }
        }

        // Lighting: stream frames for animated effects, and write static
        // colours once (again whenever the mouse wakes up).
        if new.awake {
            if let Some(d) = &dev {
                let animate = lighting::animated(&light);
                let due = Instant::now() >= next_frame;
                let mut res = if light.mode == LightMode::OnboardRainbow {
                    if light_dirty { d.send(CMD_ONBOARD_RAINBOW, &[0xFF]).map(|_| ()) } else { Ok(()) }
                } else if light_dirty || (animate && due) {
                    let frame = lighting::render(&light, t0.elapsed().as_secs_f32());
                    write_frame(d, &frame).map(|_| ())
                } else {
                    Ok(())
                };
                if light_dirty && res.is_ok() {
                    // Keep the mouse's click flash in sync (e.g. after it wakes up).
                    res = d.send(CMD_REACTIVE, &lighting::reactive_payload(&light)).map(|_| ());
                }
                if animate && due {
                    next_frame = Instant::now() + FRAME_INTERVAL;
                }
                light_dirty = false;
                if res.is_err() {
                    dev = None;
                    new = DeviceStatus::default();
                }
            }
        }

        if new != status {
            status = new;
            on_status(status.clone());
        }
    }
}

fn write_buttons(d: &Open, mode: SideKeys) -> Result<(), String> {
    let table = button_table(mode);
    let mut slot = 0;
    for (page, &n) in PAGE_SLOTS.iter().enumerate() {
        let mut payload = vec![page as u8];
        for s in &table[slot..slot + n] {
            payload.extend_from_slice(s);
        }
        slot += n;
        match d.send(CMD_BUTTONS, &payload) {
            Ok(Some(_)) => {}
            Ok(None) => return Err(format!("The mouse didn't confirm page {}", page + 1)),
            Err(e) => return Err(e.to_string()),
        }
    }
    d.send(CMD_SAVE, &[0x00]).map_err(|e| e.to_string())?;
    Ok(())
}

fn polling_code(hz: u32) -> u8 {
    match hz {
        125 => 0x03,
        250 => 0x02,
        500 => 0x01,
        _ => 0x00,
    }
}
