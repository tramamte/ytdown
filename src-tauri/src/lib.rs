#[allow(dead_code)] // commands registered in Task 7
mod download;
mod args;
#[allow(dead_code)] // consumed by commands in Task 7
mod settings;
#[allow(dead_code)] // consumed by download flow (Task 6) and commands (Task 7)
mod deps;
mod progress;
mod url;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
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
