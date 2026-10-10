const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const dialogApi = window.__TAURI__.dialog;
const $ = (s) => document.querySelector(s);
const items = new Map(); // id -> <li>

// ---- helpers ----
function showErr(el, msg) {
  el.textContent = msg;
  el.hidden = !msg;
}
function flash(input, msg) {
  showErr(input.parentElement.querySelector(".err"), String(msg));
}
function askPlaylist() {
  const d = $("#playlist-dialog");
  return new Promise((resolve) => {
    d.addEventListener("close", () => resolve(d.returnValue || null), { once: true });
    d.returnValue = "";
    d.showModal();
  });
}

// ---- banner / notice ----
// URL input gets focus on launch (once enabled), shortcut and window activation — unless a dialog is open
const focusUrl = () => { if (!document.querySelector("dialog[open]")) $("#url").focus(); };

function setBanner({ state, message }) {
  const b = $("#banner");
  const usable = state === "ready" || state === "update";
  // "notice": blue callout without the spinner (base #banner style)
  b.className = state === "error" ? "error" : state === "ready" ? "warn" : state === "update" ? "notice" : "info";
  b.hidden = state === "ready" && !message;
  $("#banner-text").textContent = message || "";
  $("#banner-retry").hidden = state !== "error";
  $("#banner-close").hidden = !usable;
  const wasDisabled = $("#url").disabled;
  document.querySelectorAll(".url").forEach((i) => (i.disabled = !usable));
  if (usable && wasDisabled) focusUrl();
}
$("#banner-retry").onclick = () => invoke("ensure_deps").catch(() => {});
$("#banner-close").onclick = () => ($("#banner").hidden = true);
$("#notice-close").onclick = () => ($("#notice").hidden = true);
function notice(msg) {
  $("#notice-text").textContent = msg;
  $("#notice").hidden = false;
}

// ---- downloads ----
const LABEL = { queued: "Queued", running: "Downloading", done: "Done", error: "Failed", canceled: "Canceled" };
// decimal units like macOS Finder
function formatSize(n) {
  if (n < 1000) return `${n} B`;
  for (const unit of ["KB", "MB", "GB"]) {
    n /= 1000;
    if (n < 1000 || unit === "GB") return `${n.toFixed(1)} ${unit}`;
  }
}
function render(u) {
  let li = items.get(u.id);
  if (!li) {
    li = document.createElement("li");
    li.innerHTML = '<span class="title"></span><button class="cancel" aria-label="Cancel download" title="Cancel">&times;</button>' +
      '<span class="meta"></span><progress max="100"></progress><span class="msg"></span>';
    li.querySelector(".cancel").onclick = () => invoke("cancel_download", { id: u.id });
    items.set(u.id, li);
    $("#list").prepend(li);
  }
  const active = u.state === "queued" || u.state === "running";
  li.dataset.state = u.state;
  li.querySelector(".title").textContent = u.title || "(untitled)";
  const label = u.state === "done" && u.size > 0 ? formatSize(u.size) : LABEL[u.state] || u.state;
  const kind = u.kind === "audio" ? "Audio" : "Video";
  let detail = label;
  if (u.state === "running") {
    const pct = `${Math.round(u.percent || 0)}%`;
    detail = u.stages > 1 && u.stage > 0 ? `${u.stream === "audio" ? "Audio" : "Video"} ${u.stage}/${u.stages} · ${pct}` : pct;
  }
  li.querySelector(".meta").textContent = `${kind} · ${detail}`;
  const p = li.querySelector("progress");
  if (u.state === "queued") p.removeAttribute("value");
  else p.value = u.state === "done" ? 100 : u.percent || 0;
  li.querySelector(".msg").textContent = u.state === "error" ? u.message || "" : "";
  li.querySelector(".cancel").hidden = !active;
}
$("#clear").onclick = () => {
  for (const [id, li] of items) {
    if (!["queued", "running"].includes(li.dataset.state)) { li.remove(); items.delete(id); }
  }
};

async function submit(input) {
  const url = input.value.trim();
  if (!url) return;
  flash(input, "");
  try {
    const k = await invoke("classify_url", { url });
    if (k === "Invalid") return flash(input, "Not a YouTube URL");
    let playlist = false;
    if (k === "Playlist") {
      const c = await askPlaylist();
      if (!c) return;
      playlist = c === "all";
    }
    input.value = "";
    await invoke("start_download", { url, kind: $("#audio-only").checked ? "audio" : "video", playlist });
  } catch (e) {
    flash(input, e);
  }
}
document.querySelectorAll(".url").forEach((i) =>
  i.addEventListener("keydown", (e) => e.key === "Enter" && submit(i)));

// ---- settings ----
let current;
const syncAq = () => { $("#s-aq").disabled = ["original", "flac", "wav"].includes($("#s-audio").value); };
$("#s-audio").addEventListener("change", syncAq);
const fill = (s) => {
  current = s;
  $("#s-dir").value = s.download_dir;
  $("#s-quality").value = s.quality ?? "";
  $("#s-container").value = s.container;
  $("#s-audio").value = s.audio_format;
  $("#s-codec").value = s.video_codec;
  $("#s-aq").value = s.audio_quality;
  $("#s-vacodec").value = s.video_audio_codec;
  $("#s-vabitrate").value = s.video_audio_bitrate;
  $("#s-varecode").value = s.video_audio_recode;
  syncAq();
  setShortcutField(s.shortcut);
};
const MAC = navigator.userAgent.includes("Mac");
$("#mac-note").hidden = !MAC;

// ---- shortcut recorder: click the field, then press the keys ----
// Stored value is a Tauri accelerator (e.g. "Control+Alt+Y"); the field shows ⌃⌥Y / Ctrl+Alt+Y.
const SYMBOL = MAC ? { CommandOrControl: "⌘", Control: "⌃", Alt: "⌥", Shift: "⇧" }
                   : { CommandOrControl: "Ctrl", Control: "Ctrl", Alt: "Alt", Shift: "Shift" };
const prettyAccel = (a) => a.split("+").map((k) => SYMBOL[k] || k).join(MAC ? "" : "+");
function setShortcutField(accel) {
  const f = $("#s-shortcut");
  f.dataset.accel = accel;
  f.value = prettyAccel(accel);
}
// e.code names are accepted by the global-hotkey parser; shorten KeyY → Y, Digit1 → 1
const keyName = (code) => code.replace(/^Key([A-Z])$/, "$1").replace(/^Digit(\d)$/, "$1");
const sc = $("#s-shortcut");
// the live global shortcut is released while recording so its own combo can be captured,
// then the saved one is restored (a newly recorded combo takes effect on Save)
sc.addEventListener("focus", () => { sc.value = "Press keys…"; invoke("suspend_shortcut").catch(() => {}); });
sc.addEventListener("blur", () => {
  setShortcutField(sc.dataset.accel || "");
  invoke("set_shortcut", { accel: current.shortcut }).catch(() => {});
});
sc.addEventListener("keydown", (e) => {
  if (e.key === "Tab") return;                       // keep keyboard navigation
  e.preventDefault();
  if (e.key === "Escape") { sc.blur(); return; }     // cancel recording, keep previous
  if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return;  // wait for the main key
  const mods = [];
  // platform order: macOS ⌃⌥⇧⌘, others Ctrl+Alt+Shift
  if (!MAC && e.ctrlKey) mods.push("CommandOrControl");
  if (MAC && e.ctrlKey) mods.push("Control");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (MAC && e.metaKey) mods.push("CommandOrControl");
  if (!mods.some((m) => m !== "Shift")) { sc.value = MAC ? "Include ⌘, ⌃ or ⌥" : "Include Ctrl or Alt"; return; }
  setShortcutField([...mods, keyName(e.code)].join("+"));
  sc.blur();
});
$("#open-settings").onclick = () => { showErr($("#s-err"), ""); fill(current); $("#settings-dialog").showModal(); };
$("#s-cancel").onclick = () => $("#settings-dialog").close();
$("#s-deps").onclick = () => invoke("update_deps").catch(() => {});
$("#s-choose").onclick = async () => {
  const dir = await dialogApi.open({ directory: true });
  if (dir) $("#s-dir").value = dir;
};
$("#settings-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const err = $("#s-err");
  const q = $("#s-quality").value;
  const s = { ...current, download_dir: $("#s-dir").value, quality: q ? Number(q) : null,
    container: $("#s-container").value, audio_format: $("#s-audio").value,
    video_codec: $("#s-codec").value, audio_quality: $("#s-aq").value,
    video_audio_codec: $("#s-vacodec").value, video_audio_bitrate: $("#s-vabitrate").value,
    video_audio_recode: $("#s-varecode").value };
  try {
    await invoke("save_settings", { s });
    const accel = $("#s-shortcut").dataset.accel || "";
    if (accel && accel !== current.shortcut) await invoke("set_shortcut", { accel });
    current = { ...s, shortcut: accel || current.shortcut };
    $("#shortcut-note").hidden = true;
    $("#settings-dialog").close();
  } catch (x) {
    showErr(err, x);
  }
});

// ---- settings keys: Ctrl+, (also ⌘, on macOS) opens; inside it Cmd/Ctrl+S saves (Esc closes natively) ----
// e.code so non-Latin input sources (e.g. Korean) still match. Returns "open" | "save" | null.
const settingsKey = (e, mac, openId) => {
  const mod = mac ? e.metaKey : e.ctrlKey;
  if (e.code === "Comma" || e.key === ",")  // key position, or the "," character on other layouts
    return (e.ctrlKey || mod) && !e.altKey && !e.shiftKey && !openId ? "open" : null;
  return mod && e.code === "KeyS" && openId === "settings-dialog" ? "save" : null;
};
document.addEventListener("keydown", (e) => {
  if (e.defaultPrevented) return;  // the shortcut recorder consumed it
  const a = settingsKey(e, MAC, document.querySelector("dialog[open]")?.id);
  if (!a) return;
  e.preventDefault();
  if (a === "open") $("#open-settings").click();
  else $("#settings-form").requestSubmit();
});

// ---- startup (listeners first, then settings, shortcut, deps) ----
(async () => {
  await listen("deps://status", ({ payload }) => setBanner(payload));
  await listen("focus-input", focusUrl);
  window.addEventListener("focus", focusUrl);
  await listen("download://update", ({ payload }) => render(payload));
  fill(await invoke("get_settings"));
  try {
    await invoke("set_shortcut", { accel: current.shortcut });
  } catch (e) {
    showErr($("#shortcut-note"), `Global shortcut unavailable: ${e}`);
    notice(`Global shortcut unavailable: ${e}`);
  }
  invoke("ensure_deps").catch(() => {});
})();
