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
function setBanner({ state, message }) {
  const b = $("#banner");
  const usable = state === "ready" || state === "update";
  // "notice": blue callout without the spinner (base #banner style)
  b.className = state === "error" ? "error" : state === "ready" ? "warn" : state === "update" ? "notice" : "info";
  b.hidden = state === "ready" && !message;
  $("#banner-text").textContent = message || "";
  $("#banner-retry").hidden = state !== "error";
  $("#banner-close").hidden = !usable;
  document.querySelectorAll(".url").forEach((i) => (i.disabled = !usable));
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
    li.innerHTML = '<span class="title"></span><button class="cancel">Cancel</button>' +
      '<span class="meta"></span><progress max="100"></progress><span class="msg"></span>';
    li.querySelector(".cancel").onclick = () => invoke("cancel_download", { id: u.id });
    items.set(u.id, li);
    $("#list").prepend(li);
  }
  const active = u.state === "queued" || u.state === "running";
  li.dataset.state = u.state;
  li.querySelector(".title").textContent = u.title || "(untitled)";
  const label = u.state === "done" && u.size > 0 ? formatSize(u.size) : LABEL[u.state] || u.state;
  li.querySelector(".meta").textContent = `${u.kind === "audio" ? "Audio" : "Video"} · ${label}`;
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
  syncAq();
  $("#s-shortcut").value = s.shortcut;
};
$("#mac-note").hidden = !navigator.userAgent.includes("Mac");
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
    video_codec: $("#s-codec").value, audio_quality: $("#s-aq").value };
  try {
    await invoke("save_settings", { s });
    const accel = $("#s-shortcut").value.trim();
    if (accel && accel !== current.shortcut) await invoke("set_shortcut", { accel });
    current = { ...s, shortcut: accel || current.shortcut };
    $("#shortcut-note").hidden = true;
    $("#settings-dialog").close();
  } catch (x) {
    showErr(err, x);
  }
});

// ---- startup (listeners first, then settings, shortcut, deps) ----
(async () => {
  await listen("deps://status", ({ payload }) => setBanner(payload));
  await listen("focus-input", () => $("#url").focus());
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
