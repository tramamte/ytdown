#[allow(dead_code)] // consumed by download flow in Task 6
mod args;
#[allow(dead_code)] // consumed by commands in Task 7
mod settings;
#[allow(dead_code)] // consumed by download flow in Task 6
mod progress;
#[allow(dead_code)] // consumed by download flow in Task 6
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
