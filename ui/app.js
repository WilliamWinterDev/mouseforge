const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (s) => document.querySelector(s);
const el = (tag, props = {}, ...kids) => {
  const n = Object.assign(document.createElement(tag), props);
  for (const k of kids.flat()) if (k != null) n.append(k);
  return n;
};

let cfg = null;
let status = null;
let selProfile = 0;
let selTrigger = null;
let runningApps = [];

// The Aerox 9 has no classic back/forward thumb buttons (the side panel replaces them).
const TRIGGERS_MAIN = [["middle", "Wheel click"]];
const TRIGGERS_SIDE = Array.from({ length: 12 }, (_, i) => [`side${i + 1}`, `Side ${i + 1}`]);
const TRIGGER_NAMES = Object.fromEntries([...TRIGGERS_MAIN, ...TRIGGERS_SIDE]);

const ACTION_TYPES = [
  ["default", "Normal function"],
  ["disabled", "Disabled"],
  ["keys", "Key / shortcut"],
  ["mouse_button", "Mouse button"],
  ["media", "Media & volume"],
  ["dpi_next", "Next DPI preset"],
  ["dpi_prev", "Previous DPI preset"],
  ["dpi_set", "Set DPI"],
  ["dpi_hold", "Sniper (DPI while held)"],
  ["macro", "Macro"],
  ["launch", "Open app / file / URL"],
  ["text", "Type text"],
];
const MEDIA = [
  ["play_pause", "Play / pause"], ["next", "Next track"], ["prev", "Previous track"], ["stop", "Stop"],
  ["volume_up", "Volume up"], ["volume_down", "Volume down"], ["mute", "Mute"],
];
const MOUSE_BUTTONS = [["left", "Left click"], ["right", "Right click"], ["middle", "Middle click"], ["back", "Back"], ["forward", "Forward"]];

// ------------------------------------------------------------------ saving

let saveTimer = null;
let savePending = false;
function scheduleSave() {
  $("#save-state").textContent = "Saving…";
  savePending = true;
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    try {
      await invoke("save_config", { config: cfg });
      $("#save-state").textContent = "All changes saved";
    } catch (e) {
      $("#save-state").textContent = "Not saved";
      toast(`Couldn't save: ${e}`);
    } finally {
      savePending = false;
    }
  }, 250);
}

function toast(msg) {
  const t = $("#toast");
  t.textContent = msg;
  t.classList.add("show");
  clearTimeout(toast.t);
  toast.t = setTimeout(() => t.classList.remove("show"), 2600);
}

// ------------------------------------------------------------------ tabs

document.querySelectorAll(".tabs button[data-tab]").forEach((b) =>
  b.addEventListener("click", () => {
    document.querySelectorAll(".tabs button").forEach((x) => x.classList.toggle("active", x === b));
    document.querySelectorAll(".tab-panel").forEach((p) => p.classList.toggle("active", p.id === `tab-${b.dataset.tab}`));
    if (b.dataset.tab === "dpi") renderDpi();
  })
);

// ------------------------------------------------------------------ status

function updateStatus(s) {
  status = s;
  const d = s.device;
  const pill = $("#pill-device");
  pill.classList.toggle("ok", d.connected && d.awake);
  pill.classList.toggle("sleep", d.connected && !d.awake);
  $("#device-text").textContent = !d.connected ? "Mouse not found" : d.awake ? d.model : "Mouse asleep";

  const lvl = d.battery;
  const color = lvl == null ? "var(--muted)" : lvl <= 10 ? "var(--bad)" : lvl <= 25 ? "var(--warn)" : "var(--good)";
  $("#battery-text").textContent = lvl == null ? "--" : `${lvl}%${d.charging ? " ⚡" : ""}`;
  $("#batt-fill").style.width = `${lvl ?? 0}%`;
  $("#batt-fill").style.background = color;
  $("#big-batt-fill").style.width = `${lvl ?? 0}%`;
  $("#big-batt-fill").style.background = color;
  $("#big-batt-text").textContent = lvl == null ? "--" : `${lvl}%`;
  $("#big-batt-sub").textContent = !d.connected
    ? "Plug in the 2.4 GHz receiver"
    : !d.awake ? "The mouse is asleep. Move it to wake it up."
    : d.charging ? "Charging" : "On battery";

  $("#dpi-text").textContent = d.live_dpi ?? "--";
  $("#profile-text").textContent = s.profile;
  $("#fg-text").textContent = s.foreground ? `· ${s.foreground}` : "";

  // The DPI button / bindings may have moved the active preset; keep our copy in sync
  // so the next save doesn't undo it.
  if (cfg && !savePending) {
    const target = s.profile_index > 0 && cfg.profiles[s.profile_index]?.dpi ? cfg.profiles[s.profile_index].dpi : cfg.dpi;
    if (JSON.stringify(target.presets) === JSON.stringify(s.dpi.presets) && target.active !== s.dpi.active) {
      target.active = s.dpi.active;
      if ($("#tab-dpi").classList.contains("active")) renderDpi();
    }
  }
  if (cfg) renderProfileList();
}

// ------------------------------------------------------------------ profiles

function renderProfileList() {
  const ul = $("#profile-list");
  ul.replaceChildren(
    ...cfg.profiles.map((p, i) =>
      el("li", { className: i === selProfile ? "selected" : "", onclick: () => selectProfile(i) },
        el("span", { textContent: p.name || "(unnamed)" }),
        status && status.profile_index === i ? el("span", { className: "live", textContent: "ACTIVE" }) : null)
    )
  );
}

function selectProfile(i) {
  selProfile = i;
  selTrigger = null;
  renderProfile();
}

function renderProfile() {
  renderProfileList();
  const p = cfg.profiles[selProfile];
  const isDefault = selProfile === 0;
  $("#profile-name").value = p.name;
  $("#delete-profile").style.display = isDefault ? "none" : "";
  $("#apps-block").style.display = isDefault ? "none" : "";
  $("#dpi-override-row").style.display = isDefault ? "none" : "";
  $("#dpi-override").checked = !!p.dpi;
  $("#app-chips").replaceChildren(
    ...p.apps.map((a) => el("span", { className: "chip" }, a,
      el("button", { textContent: "×", title: "Remove", onclick: () => { p.apps = p.apps.filter((x) => x !== a); scheduleSave(); renderProfile(); } })))
  );
  if (!p.apps.length && !isDefault) $("#app-chips").append(el("span", { className: "muted", textContent: "No apps yet. This profile won't activate until you add one." }));
  renderTriggers();
  renderEditor();
}

$("#profile-name").addEventListener("input", (e) => {
  cfg.profiles[selProfile].name = e.target.value;
  renderProfileList();
  scheduleSave();
});
$("#add-profile").addEventListener("click", () => {
  cfg.profiles.push({ name: `Profile ${cfg.profiles.length}`, apps: [], bindings: {}, dpi: null });
  selectProfile(cfg.profiles.length - 1);
  scheduleSave();
  $("#profile-name").select();
});
$("#delete-profile").addEventListener("click", () => {
  if (selProfile === 0) return;
  if (!confirm(`Delete the profile "${cfg.profiles[selProfile].name}"?`)) return;
  cfg.profiles.splice(selProfile, 1);
  selectProfile(0);
  scheduleSave();
});
$("#dpi-override").addEventListener("change", (e) => {
  const p = cfg.profiles[selProfile];
  p.dpi = e.target.checked ? structuredClone(cfg.dpi) : null;
  scheduleSave();
  if (e.target.checked) toast("Now open the DPI tab to set this profile's presets");
});

async function loadRunningApps() {
  runningApps = await invoke("running_apps");
  $("#app-picker").replaceChildren(el("option", { value: "", textContent: "Pick a running app…" }),
    ...runningApps.map((a) => el("option", { value: a, textContent: a })));
}
$("#app-refresh").addEventListener("click", loadRunningApps);
$("#app-add").addEventListener("click", () => {
  const v = ($("#app-manual").value.trim() || $("#app-picker").value).toLowerCase();
  if (!v) return;
  const exe = v.endsWith(".exe") ? v : `${v}.exe`;
  const p = cfg.profiles[selProfile];
  if (!p.apps.includes(exe)) p.apps.push(exe);
  $("#app-manual").value = "";
  $("#app-picker").value = "";
  scheduleSave();
  renderProfile();
});

// ------------------------------------------------------------------ triggers

function describe(a) {
  if (!a) return null;
  switch (a.type) {
    case "default": return null;
    case "disabled": return "Disabled";
    case "keys": return a.combo || "(no key)";
    case "mouse_button": return MOUSE_BUTTONS.find((m) => m[0] === a.button)?.[1] ?? a.button;
    case "media": return MEDIA.find((m) => m[0] === a.key)?.[1] ?? a.key;
    case "dpi_next": return "DPI +";
    case "dpi_prev": return "DPI −";
    case "dpi_set": return `DPI ${a.dpi}`;
    case "dpi_hold": return `Sniper ${a.dpi}`;
    case "macro": return `Macro (${a.steps.length} steps)`;
    case "launch": return `Open ${a.path.split(/[\\/]/).pop() || "…"}`;
    case "text": return `Type "${a.text.slice(0, 14)}${a.text.length > 14 ? "…" : ""}"`;
  }
  return a.type;
}

function triggerButton([id, name]) {
  const own = cfg.profiles[selProfile].bindings[id];
  const ownDesc = describe(own);
  const inherited = selProfile > 0 && !ownDesc ? describe(cfg.profiles[0].bindings[id]) : null;
  const cls = ["trigger", selTrigger === id ? "selected" : "", ownDesc ? "bound" : inherited ? "inherited" : ""].join(" ");
  return el("button", { className: cls, onclick: () => { selTrigger = id; renderTriggers(); renderEditor(); } },
    el("span", { className: "t-name", textContent: name }),
    el("span", { className: "t-act", textContent: ownDesc ?? (inherited ? `↳ ${inherited}` : "Normal") }));
}

function renderTriggers() {
  $("#main-triggers").replaceChildren(...TRIGGERS_MAIN.map(triggerButton));
  $("#side-triggers").replaceChildren(...TRIGGERS_SIDE.map(triggerButton));
}

// ------------------------------------------------------------------ editor

function defaultsFor(type) {
  switch (type) {
    case "keys": return { type, combo: "" };
    case "mouse_button": return { type, button: "middle" };
    case "media": return { type, key: "play_pause" };
    case "dpi_set": return { type, dpi: 1600 };
    case "dpi_hold": return { type, dpi: 400 };
    case "macro": return { type, steps: [] };
    case "launch": return { type, path: "", args: "" };
    case "text": return { type, text: "" };
    default: return { type };
  }
}

function setAction(a) {
  const b = cfg.profiles[selProfile].bindings;
  if (a.type === "default") delete b[selTrigger];
  else b[selTrigger] = a;
  scheduleSave();
  renderTriggers();
}

function select(options, value, onchange) {
  const s = el("select", { onchange: (e) => onchange(e.target.value) },
    ...options.map(([v, l]) => el("option", { value: v, textContent: l })));
  s.value = value;
  return s;
}

function field(label, control) {
  return el("label", { className: "field" }, el("span", { textContent: label }), control);
}

function renderEditor() {
  const body = $("#editor-body");
  if (!selTrigger) {
    $("#editor-title").textContent = "Select a button";
    body.className = "muted";
    body.replaceChildren("Click a button above to change what it does.");
    return;
  }
  body.className = "";
  $("#editor-title").textContent = `${TRIGGER_NAMES[selTrigger]}${selProfile ? ` · ${cfg.profiles[selProfile].name}` : ""}`;
  const a = cfg.profiles[selProfile].bindings[selTrigger] ?? { type: "default" };
  const types = ACTION_TYPES.map(([v, l]) => [v, v === "default" && selProfile > 0 ? "Same as Default profile" : l]);
  const kids = [field("Action", select(types, a.type, (t) => { setAction(defaultsFor(t)); renderEditor(); }))];

  const num = (key, min, max, step) => el("input", {
    type: "number", min, max, step, value: a[key],
    onchange: (e) => { a[key] = Math.round(Math.min(max, Math.max(min, +e.target.value || min)) / step) * step; e.target.value = a[key]; setAction(a); },
  });

  switch (a.type) {
    case "keys":
      kids.push(field("Shortcut (held for as long as you hold the button)", recorder(a.combo, (c) => { a.combo = c; setAction(a); })));
      break;
    case "mouse_button":
      kids.push(field("Acts as", select(MOUSE_BUTTONS, a.button, (v) => { a.button = v; setAction(a); })));
      break;
    case "media":
      kids.push(field("Media key", select(MEDIA, a.key, (v) => { a.key = v; setAction(a); })));
      break;
    case "dpi_set":
      kids.push(field("DPI (100 – 18000)", num("dpi", 100, 18000, 100)));
      kids.push(el("p", { className: "hint", textContent: "Stays until you switch profiles or use another DPI button." }));
      break;
    case "dpi_hold":
      kids.push(field("DPI while held (100 – 18000)", num("dpi", 100, 18000, 100)));
      break;
    case "dpi_next": case "dpi_prev":
      kids.push(el("p", { className: "hint", textContent: "Cycles through the presets on the DPI tab (or this profile's own presets)." }));
      break;
    case "macro":
      kids.push(macroEditor(a));
      break;
    case "launch":
      kids.push(field("Program, file or URL", el("input", { type: "text", value: a.path, placeholder: "C:\\Program Files\\…\\app.exe  or  https://…", onchange: (e) => { a.path = e.target.value; setAction(a); } })));
      kids.push(field("Arguments (optional)", el("input", { type: "text", value: a.args, onchange: (e) => { a.args = e.target.value; setAction(a); } })));
      break;
    case "text":
      kids.push(field("Text to type", el("textarea", { value: a.text, oninput: (e) => { a.text = e.target.value; setAction(a); } })));
      break;
    case "disabled":
      kids.push(el("p", { className: "hint", textContent: "The button does nothing at all." }));
      break;
    case "default":
      kids.push(el("p", { className: "hint", textContent: selProfile ? "Uses whatever this button does in the Default profile." : "The button does what it normally does." }));
      break;
  }
  body.replaceChildren(...kids);
}

// --- key recorder

const CODE_NAMES = {
  Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Semicolon: ";", Quote: "'", Comma: ",", Period: ".",
  Slash: "/", Backslash: "\\", Backquote: "`", Escape: "Esc", ContextMenu: "Menu", NumpadMultiply: "NumMultiply",
  NumpadAdd: "NumAdd", NumpadSubtract: "NumSubtract", NumpadDecimal: "NumDecimal", NumpadDivide: "NumDivide", NumpadEnter: "Enter",
};
const MODS = { ControlLeft: "Ctrl", ControlRight: "Ctrl", ShiftLeft: "Shift", ShiftRight: "Shift", AltLeft: "Alt", AltRight: "Alt", MetaLeft: "Win", MetaRight: "Win" };

function codeToName(code) {
  if (CODE_NAMES[code]) return CODE_NAMES[code];
  let m;
  if ((m = code.match(/^Key([A-Z])$/))) return m[1];
  if ((m = code.match(/^Digit(\d)$/))) return m[1];
  if ((m = code.match(/^Numpad(\d)$/))) return `Num${m[1]}`;
  if ((m = code.match(/^Arrow(\w+)$/))) return m[1];
  if (/^F\d+$/.test(code)) return code;
  return code; // Space, Enter, Tab, Backspace, Delete, Home, End, PageUp, PageDown, PrintScreen, CapsLock, Insert…
}

function recorder(value, onchange) {
  const input = el("input", { type: "text", value, placeholder: "e.g. Ctrl+Shift+T", onchange: (e) => onchange(e.target.value.trim()) });
  let recording = false, mods = new Set(), main = null;
  const btn = el("button", { className: "btn", textContent: "Record", type: "button" });
  const stop = () => { recording = false; btn.textContent = "Record"; input.classList.remove("recording"); };
  const order = ["Ctrl", "Shift", "Alt", "Win"];
  const combo = () => [...order.filter((m) => mods.has(m)), main].filter(Boolean).join("+");
  btn.onclick = () => {
    recording = !recording;
    if (recording) { mods = new Set(); main = null; btn.textContent = "Press keys…"; input.classList.add("recording"); input.focus(); }
    else stop();
  };
  input.addEventListener("keydown", (e) => {
    if (!recording) return;
    e.preventDefault();
    if (MODS[e.code]) mods.add(MODS[e.code]);
    else main = codeToName(e.code);
    input.value = combo();
    if (main) { onchange(input.value); stop(); }
  });
  input.addEventListener("keyup", (e) => {
    if (!recording) return;
    e.preventDefault();
    // A lone modifier (e.g. just Shift) is a valid binding too.
    if (!main && MODS[e.code]) { input.value = combo(); onchange(input.value); stop(); }
  });
  input.addEventListener("blur", stop);
  return el("div", { className: "row gap" }, Object.assign(input, { className: "recorder grow" }), btn);
}

// --- macro editor

const STEP_TYPES = [["tap", "Press shortcut"], ["down", "Hold key"], ["up", "Release key"], ["delay", "Wait (ms)"], ["text", "Type text"]];

function macroEditor(a) {
  const wrap = el("div");
  const render = () => {
    const list = el("div", { className: "steps" },
      ...a.steps.map((s, i) => {
        const valKey = s.type === "tap" ? "combo" : s.type === "delay" ? "ms" : s.type === "text" ? "text" : "key";
        const val = el("input", {
          type: s.type === "delay" ? "number" : "text", value: s[valKey] ?? "",
          placeholder: { tap: "Ctrl+C", down: "Shift", up: "Shift", delay: "50", text: "hello" }[s.type],
          onchange: (e) => { s[valKey] = s.type === "delay" ? Math.max(0, +e.target.value || 0) : e.target.value; setAction(a); },
        });
        return el("div", { className: "step" },
          el("span", { className: "num", textContent: i + 1 }),
          select(STEP_TYPES, s.type, (t) => { a.steps[i] = { type: t, ...(t === "delay" ? { ms: 50 } : t === "tap" ? { combo: "" } : t === "text" ? { text: "" } : { key: "" }) }; setAction(a); render(); }),
          val,
          el("button", { className: "icon-btn", textContent: "✕", title: "Remove step", onclick: () => { a.steps.splice(i, 1); setAction(a); render(); } }));
      }));
    wrap.replaceChildren(
      el("span", { className: "label", textContent: "Steps" }),
      a.steps.length ? list : el("p", { className: "hint", textContent: "No steps yet." }),
      el("button", { className: "btn small", textContent: "+ Add step", onclick: () => { a.steps.push({ type: "tap", combo: "" }); setAction(a); render(); } }),
      el("p", { className: "hint", textContent: "Key names: letters, digits, F1–F24, Ctrl, Shift, Alt, Win, Enter, Esc, Tab, Space, Up/Down/Left/Right, Home, End, PageUp, PageDown, Delete…" })
    );
  };
  render();
  return wrap;
}

// ------------------------------------------------------------------ DPI

function dpiTarget() {
  const p = cfg.profiles[selProfile];
  return selProfile > 0 && p.dpi ? p.dpi : cfg.dpi;
}

function renderDpi() {
  const d = dpiTarget();
  const own = d !== cfg.dpi;
  $("#dpi-scope").textContent = own ? `· ${cfg.profiles[selProfile].name} profile` : "· used by all profiles without their own";
  $("#dpi-add").disabled = d.presets.length >= 5;
  $("#dpi-list").replaceChildren(...d.presets.map((v, i) => {
    const slider = el("input", { type: "range", min: 100, max: 18000, step: 100, value: v });
    const box = el("input", { type: "number", min: 100, max: 18000, step: 100, value: v });
    const set = (nv, save) => {
      nv = Math.round(Math.min(18000, Math.max(100, +nv || 100)) / 100) * 100;
      d.presets[i] = nv; slider.value = nv; box.value = nv;
      if (save) scheduleSave();
    };
    slider.oninput = () => set(slider.value, false);
    slider.onchange = () => set(slider.value, true);
    box.onchange = () => set(box.value, true);
    return el("div", { className: "dpi-row" },
      el("button", { className: `radio ${i === d.active ? "on" : ""}`, title: "Make active", onclick: () => { d.active = i; scheduleSave(); renderDpi(); } }),
      slider, box,
      el("button", { className: "icon-btn", textContent: "✕", title: "Remove preset", disabled: d.presets.length <= 1,
        onclick: () => { d.presets.splice(i, 1); if (d.active >= d.presets.length) d.active = d.presets.length - 1; scheduleSave(); renderDpi(); } }));
  }));
  $("#polling").replaceChildren(...[125, 250, 500, 1000].map((hz) =>
    el("button", { className: cfg.polling_rate === hz ? "on" : "", textContent: `${hz} Hz`, onclick: () => { cfg.polling_rate = hz; scheduleSave(); renderDpi(); } })));
}

$("#dpi-add").addEventListener("click", () => {
  const d = dpiTarget();
  if (d.presets.length >= 5) return;
  d.presets.push(Math.min(18000, (d.presets.at(-1) ?? 800) * 2));
  scheduleSave();
  renderDpi();
});
$("#save-mouse").addEventListener("click", async () => {
  await invoke("save_to_mouse");
  toast("Saved to the mouse's onboard memory");
});

// ------------------------------------------------------------------ lighting

const LIGHT_MODES = [
  ["off", "Off", "Lights off while MouseForge is running."],
  ["static", "Static", "A fixed colour for each zone."],
  ["breathe", "Breathe", "The zone colours slowly fade in and out."],
  ["color_shift", "Colour shift", "Cycles through the rainbow, flowing down the mouse."],
  ["onboard_rainbow", "Mouse's own rainbow", "The mouse's built-in rainbow. It keeps running even when MouseForge is closed."],
];
const ZONE_NAMES = ["Top", "Middle", "Bottom"];

function renderLighting() {
  const l = cfg.lighting;
  $("#light-mode").replaceChildren(...LIGHT_MODES.map(([id, label]) =>
    el("button", { className: l.mode === id ? "on" : "", textContent: label, onclick: () => { l.mode = id; scheduleSave(); renderLighting(); } })));
  $("#light-mode-hint").textContent = LIGHT_MODES.find((m) => m[0] === l.mode)?.[2] ?? "";

  const usesColours = l.mode === "static" || l.mode === "breathe";
  $("#light-colours-card").classList.toggle("disabled", !usesColours);
  $("#zone-pickers").replaceChildren(...l.zones.map((c, i) =>
    el("label", {}, el("span", { textContent: ZONE_NAMES[i] }),
      el("input", { type: "color", value: c, oninput: (e) => { l.zones[i] = e.target.value; scheduleSave(); paintPreview(); } }))));

  $("#light-bright").value = l.brightness;
  $("#light-bright-val").textContent = `${l.brightness}%`;
  $("#light-speed").value = l.speed_secs;
  $("#light-speed-val").textContent = `${l.speed_secs} s`;
  $("#light-speed-row").style.display = l.mode === "breathe" || l.mode === "color_shift" ? "" : "none";
  $("#light-reactive").checked = !!l.reactive;
  $("#light-reactive-colour").value = l.reactive ?? "#ffffff";
  $("#light-reactive-colour").disabled = !l.reactive;

  $("#light-idle").replaceChildren(...[["rainbow", "Rainbow"], ["off", "Lights off"]].map(([id, label]) =>
    el("button", { className: l.idle === id ? "on" : "", textContent: label, onclick: () => { l.idle = id; scheduleSave(); renderLighting(); } })));
  paintPreview();
}

function paintPreview() {
  const l = cfg.lighting;
  const k = l.brightness / 100;
  l.zones.forEach((c, i) => {
    const z = $(`#mp-${i}`);
    const on = l.mode === "static" || l.mode === "breathe";
    const rainbow = l.mode === "color_shift" || l.mode === "onboard_rainbow";
    z.style.background = rainbow ? "linear-gradient(#ff3b3b, #ffd23b, #3bff6e, #3bb8ff, #b43bff)" : on ? c : "transparent";
    z.style.opacity = on || rainbow ? Math.max(0.15, k) : 1;
    z.style.boxShadow = on ? `0 0 ${14 * k}px ${c}` : "none";
    z.style.animation = l.mode === "breathe" ? `mf-breathe ${l.speed_secs}s ease-in-out infinite`
      : rainbow ? `mf-hue ${l.mode === "color_shift" ? l.speed_secs : 6}s linear infinite` : "none";
  });
}

$("#light-bright").addEventListener("input", (e) => { cfg.lighting.brightness = +e.target.value; $("#light-bright-val").textContent = `${e.target.value}%`; scheduleSave(); paintPreview(); });
$("#light-speed").addEventListener("input", (e) => { cfg.lighting.speed_secs = +e.target.value; $("#light-speed-val").textContent = `${e.target.value} s`; scheduleSave(); paintPreview(); });
$("#light-reactive").addEventListener("change", (e) => { cfg.lighting.reactive = e.target.checked ? $("#light-reactive-colour").value : null; scheduleSave(); renderLighting(); });
for (const ev of ["input", "change"]) {
  $("#light-reactive-colour").addEventListener(ev, (e) => { if (cfg.lighting.reactive) { cfg.lighting.reactive = e.target.value; scheduleSave(); } });
}
$("#light-same").addEventListener("click", () => { const l = cfg.lighting; l.zones = [l.zones[0], l.zones[0], l.zones[0]]; scheduleSave(); renderLighting(); });

// ------------------------------------------------------------------ power

const SLEEP_OPTIONS = [1, 2, 3, 5, 10, 15, 20].map((m) => [m, `${m} minute${m > 1 ? "s" : ""}`]);
const DIM_OPTIONS = [[5, "5 seconds"], [10, "10 seconds"], [15, "15 seconds"], [30, "30 seconds"], [60, "1 minute"], [120, "2 minutes"], [300, "5 minutes"], [600, "10 minutes"], [1200, "20 minutes"]];

function renderPower() {
  const p = cfg.power;
  const fill = (sel, opts, val, unit) => {
    const list = opts.some(([v]) => v === val) ? opts : [...opts, [val, `${val} ${unit}`]].sort((a, b) => a[0] - b[0]);
    sel.replaceChildren(...list.map(([v, l]) => el("option", { value: v, textContent: l })));
    sel.value = String(val);
  };
  fill($("#sleep-min"), SLEEP_OPTIONS, p.sleep_minutes, "minutes");
  fill($("#dim-sec"), DIM_OPTIONS, p.dim_seconds, "seconds");
}
$("#sleep-min").addEventListener("change", (e) => { cfg.power.sleep_minutes = +e.target.value; scheduleSave(); toast("Sleep timer saved to the mouse"); });
$("#dim-sec").addEventListener("change", (e) => { cfg.power.dim_seconds = +e.target.value; scheduleSave(); toast("Dim timer saved to the mouse"); });

// ------------------------------------------------------------------ battery

function renderBattery() {
  const b = cfg.battery;
  b.warn_levels.sort((x, y) => y - x);
  $("#warn-chips").replaceChildren(...b.warn_levels.map((w) => el("span", { className: "chip" }, `${w}%`,
    el("button", { textContent: "×", onclick: () => { b.warn_levels = b.warn_levels.filter((x) => x !== w); scheduleSave(); renderBattery(); } }))));
  $("#notify-full").checked = b.notify_full;
  $("#poll-secs").value = String(b.poll_secs);
  if ($("#poll-secs").value !== String(b.poll_secs)) $("#poll-secs").append(el("option", { value: b.poll_secs, textContent: `${b.poll_secs} seconds`, selected: true }));
}
$("#warn-add").addEventListener("click", () => {
  const v = Math.round(+$("#warn-new").value);
  if (!(v >= 1 && v <= 99)) return toast("Pick a level between 1 and 99");
  if (!cfg.battery.warn_levels.includes(v)) cfg.battery.warn_levels.push(v);
  $("#warn-new").value = "";
  scheduleSave();
  renderBattery();
});
$("#notify-full").addEventListener("change", (e) => { cfg.battery.notify_full = e.target.checked; scheduleSave(); });
$("#poll-secs").addEventListener("change", (e) => { cfg.battery.poll_secs = +e.target.value; scheduleSave(); });
$("#test-notify").addEventListener("click", () => invoke("test_notification"));
$("#batt-refresh").addEventListener("click", () => { invoke("refresh_battery"); toast("Asking the mouse…"); });

// ------------------------------------------------------------------ setup

$("#autostart").addEventListener("change", async (e) => {
  try { await invoke("set_autostart", { enabled: e.target.checked }); toast(e.target.checked ? "MouseForge will start with Windows" : "Autostart turned off"); }
  catch (err) { e.target.checked = !e.target.checked; toast(String(err)); }
});

for (const [id, mode, done] of [
  ["#side-fkeys", "fkeys", "Side panel now sends F13–F24. Press the buttons below to check."],
  ["#side-numbers", "numbers", "Side panel restored to number keys"],
]) {
  $(id).addEventListener("click", async (e) => {
    e.target.disabled = true;
    try { await invoke("set_side_keys", { mode }); toast(done); }
    catch (err) { toast(`Couldn't update the mouse: ${err}`); }
    finally { e.target.disabled = false; }
  });
}

const TESTER =[["middle", "Wheel"], ...Array.from({ length: 12 }, (_, i) => [`F${13 + i}`, `Side ${i + 1}`])];
$("#tester").replaceChildren(...TESTER.map(([id, label]) => el("div", { id: `t-${id}` }, el("strong", { textContent: label }), el("br"), el("span", { className: "muted", textContent: id.startsWith("F") ? id : "" }))));
function flash(id, text) {
  const n = document.getElementById(`t-${id}`);
  if (n) { n.classList.add("hit"); setTimeout(() => n.classList.remove("hit"), 350); }
  $("#tester-last").textContent = `Last input: ${text}`;
}
window.addEventListener("keydown", (e) => {
  if (!$("#tab-setup").classList.contains("active")) return;
  if (/^F(1[3-9]|2[0-4])$/.test(e.code)) { e.preventDefault(); flash(e.code, `${e.code} (side button)`); }
  else if (!e.target.matches("input, textarea")) flash("", `${e.code}${/^Digit|^Minus|^Equal/.test(e.code) ? " - if this came from the side panel, it still needs the one-time setup" : ""}`);
});
window.addEventListener("mousedown", (e) => {
  const id = { 1: "middle", 3: "back", 4: "forward" }[e.button];
  if (id && $("#tab-setup").classList.contains("active")) flash(id, TRIGGER_NAMES[id]);
  if (e.button === 3 || e.button === 4 || e.button === 1) e.preventDefault(); // no history navigation / autoscroll
});
window.addEventListener("mouseup", (e) => { if (e.button === 3 || e.button === 4) e.preventDefault(); });

// ------------------------------------------------------------------ boot

(async function boot() {
  cfg = await invoke("get_config");
  for (const p of cfg.profiles) p.bindings ??= {};
  renderProfile();
  renderBattery();
  renderLighting();
  renderPower();
  updateStatus(await invoke("get_status"));
  $("#autostart").checked = await invoke("get_autostart");
  loadRunningApps();
  await listen("status", (e) => updateStatus(e.payload));
})();
