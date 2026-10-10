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
// ↑/↓ move between the playlist choices (wraps around); Tab is blocked; Esc cancels
$("#playlist-dialog").addEventListener("keydown", (e) => {
  if (e.key === "Tab") { e.preventDefault(); return; }
  // explicit: don't rely on the WebView's native <dialog> Esc handling
  if (e.key === "Escape") { e.preventDefault(); $("#playlist-dialog").close(""); return; }
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const opts = [...document.querySelectorAll("#playlist-dialog .opt:not([hidden])")];
  const i = opts.indexOf(document.activeElement);
  e.preventDefault();
  opts[(i + (e.key === "ArrowDown" ? 1 : -1) + opts.length) % opts.length].focus();
});
function askPlaylist(only) {
  const d = $("#playlist-dialog");
  $("#opt-single").hidden = only;
  $("#playlist-q").textContent = only ? "This link is a playlist." : "Download just this video, or the whole playlist?";
  return new Promise((resolve) => {
    d.addEventListener("close", () => resolve(d.returnValue || null), { once: true });
    d.returnValue = "";
    d.showModal();
    if (only) d.querySelector('[value="all"]').focus();
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
  const units = ["KB", "MB", "GB"];
  let i = 0;
  n /= 1000;
  while (i < 2 && n.toFixed(1) >= 1000) { n /= 1000; i++; }  // pick the unit after rounding
  return `${n.toFixed(1)} ${units[i]}`;
}
// running detail: "Video 1/2 · 40%" while merging streams, else "40%"
function runDetail(u) {
  const pct = `${Math.round(u.percent || 0)}%`;
  return u.stages > 1 && u.stage > 0 ? `${u.stream === "audio" ? "Audio" : "Video"} ${u.stage}/${u.stages} · ${pct}` : pct;
}
const status = (u) => u.state === "running" ? runDetail(u)
  : u.state === "done" && u.size > 0 ? formatSize(u.size) : LABEL[u.state] || u.state;
const kindLabel = (u) => (u.kind === "audio" ? "Audio" : "Video");
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
  let detail = status(u);
  if (u.playlist && u.state === "running") {
    const pct = `${Math.round(u.percent || 0)}%`;
    detail = u.items_total ? `${u.items_done}/${u.items_total} · ${pct}` : pct;
  } else if (u.playlist && u.state === "done") {
    detail = `${u.items_total} items` + (u.size > 0 ? ` · ${formatSize(u.size)}` : "");
  }
  li.querySelector(".meta").textContent = `${u.playlist ? "Playlist" : kindLabel(u)} · ${detail}`;
  const p = li.querySelector("progress");
  if (u.state === "queued") p.removeAttribute("value");
  else p.value = u.state === "done" ? 100 : u.percent || 0;
  // done + message: some playlist items failed
  li.querySelector(".msg").textContent = ["error", "done"].includes(u.state) ? u.message || "" : "";
  li.querySelector(".cancel").hidden = !active;
}
// playlist item: child row nested in its job's <li>, appended as items start
function renderItem(c) {
  const parent = items.get(c.job_id);
  if (!parent) return;
  let ul = parent.querySelector(".items");
  if (!ul) {
    ul = document.createElement("ul");
    ul.className = "items";
    parent.append(ul);
  }
  let li = [...ul.children].find((x) => x.dataset.index === String(c.index));
  if (!li) {
    li = document.createElement("li");
    li.dataset.index = c.index;
    li.innerHTML = '<span class="title"></span><span class="meta"></span><progress max="100"></progress><span class="msg"></span>';
    ul.append(li);
  }
  li.dataset.state = c.state;
  li.querySelector(".title").textContent = c.title || `Item ${c.index}`;
  li.querySelector(".meta").textContent = `${kindLabel(c)} · ${status(c)}`;
  li.querySelector("progress").value = c.percent || 0;
  li.querySelector(".msg").textContent = c.state === "error" ? c.message || "" : "";
}
$("#clear").onclick = () => {
  for (const [id, li] of items) {
    if (!["queued", "running"].includes(li.dataset.state)) { li.remove(); items.delete(id); }
  }
};

async function submit(input, audioOnce = false) {
  const url = input.value.trim();
  if (!url) return;
  flash(input, "");
  try {
    const k = await invoke("classify_url", { url });
    if (k === "Invalid") return flash(input, "Not a YouTube URL");
    let playlist = false;
    if (k === "Playlist" || k === "PlaylistOnly") {
      const c = await askPlaylist(k === "PlaylistOnly");
      if (!c) return;
      playlist = c === "all";
    }
    input.value = "";
    await invoke("start_download", { url, kind: audioOnce || $("#audio-only").checked ? "audio" : "video", playlist });
  } catch (e) {
    flash(input, e);
  }
}
document.querySelectorAll(".url").forEach((i) =>
  i.addEventListener("keydown", (e) => e.key === "Enter" && !e.isComposing && submit(i, e.shiftKey)));  // Shift+Enter: audio this time

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
// e.code names the global-hotkey parser (parse_key, v0.8) accepts; shorten KeyY → Y, Digit1 → 1
const KEY_OK = /^(Key[A-Z]|Digit\d|F([1-9]|1\d|2[0-4])|Numpad(\d|Add|Decimal|Divide|Enter|Equal|Multiply|Subtract)|Arrow(Up|Down|Left|Right)|Backquote|Backslash|Bracket(Left|Right)|Comma|Equal|Minus|Period|Quote|Semicolon|Slash|Pause|Backspace|CapsLock|Enter|Space|Tab|Delete|End|Home|Insert|PageUp|PageDown|PrintScreen|ScrollLock|NumLock|Escape|AudioVolume(Up|Down|Mute)|Media(Play|Pause|PlayPause|Stop|TrackNext|TrackPrevious))$/;
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
  if (!KEY_OK.test(e.code)) { sc.value = "Key not supported"; return; }
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

// ---- Audio only: ⌘⇧A / Ctrl+Shift+A toggles (main window only); Shift+Enter in the URL field = audio once ----
document.addEventListener("keydown", (e) => {
  if (e.defaultPrevented || document.querySelector("dialog[open]")) return;
  if ((MAC ? e.metaKey : e.ctrlKey) && e.shiftKey && !e.altKey && e.code === "KeyA") {
    e.preventDefault();
    $("#audio-only").checked = !$("#audio-only").checked;
  }
});
$("#audio-hint").textContent = MAC ? "⌘⇧A toggle · ⇧↩ audio once" : "Ctrl+⇧A toggle · ⇧Enter once";

// ---- startup (listeners first, then settings, shortcut, deps) ----
(async () => {
  await listen("deps://status", ({ payload }) => setBanner(payload));
  await listen("focus-input", focusUrl);
  window.addEventListener("focus", focusUrl);
  await listen("download://update", ({ payload }) => render(payload));
  await listen("download://item", ({ payload }) => renderItem(payload));
  fill(await invoke("get_settings"));
  try {
    await invoke("set_shortcut", { accel: current.shortcut });
  } catch (e) {
    showErr($("#shortcut-note"), `Global shortcut unavailable: ${e}`);
    notice(`Global shortcut unavailable: ${e}`);
  }
  invoke("ensure_deps").catch(() => {});
})();
