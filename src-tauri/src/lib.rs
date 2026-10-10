mod args;
mod deps;
mod download;
mod progress;
mod settings;
mod url;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[tauri::command]
fn get_settings(app: AppHandle) -> settings::Settings {
  settings::load(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, mut s: settings::Settings) -> Result<(), String> {
  // The shortcut is changed only via set_shortcut; keep the stored one.
  s.shortcut = settings::load(&app).shortcut;
  settings::validate(&s)?;
  settings::save(&app, &s)
}

#[tauri::command]
fn set_shortcut(app: AppHandle, accel: String) -> Result<(), String> {
  let mut s = settings::load(&app);
  let prev = s.shortcut.clone();
  let gs = app.global_shortcut();
  let _ = gs.unregister_all();
  if let Err(e) = gs.register(accel.as_str()) {
    let _ = gs.register(prev.as_str());
    return Err(format!("Could not register shortcut {accel}: {e}"));
  }
  if s.shortcut != accel {
    s.shortcut = accel;
    settings::save(&app, &s)?;
  }
  Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(
      tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _sc, ev| {
          if ev.state() == ShortcutState::Pressed {
            if let Some(w) = app.get_webview_window("main") {
              let _ = w.show();
              let _ = w.unminimize();
              let _ = w.set_focus();
            }
            let _ = app.emit("focus-input", ());
          }
        })
        .build(),
    )
    .plugin(tauri_plugin_dialog::init())
    .manage(download::Manager::new())
    .invoke_handler(tauri::generate_handler![
      deps::ensure_deps,
      deps::update_deps,
      download::classify_url,
      download::start_download,
      download::cancel_download,
      get_settings,
      save_settings,
      set_shortcut
    ])
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while building tauri application");
}
