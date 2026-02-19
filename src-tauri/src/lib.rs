#[cfg_attr(mobile, tauri::mobile_entry_point)]

use serde::Serialize;
use tauri::command;
use xcap::Window;

#[derive(Serialize)]
struct WindowInfo {
    id: u32,
    title: String,
    app_name: String,
}

#[command]
fn get_windows() -> Result<Vec<WindowInfo>, String> {
    let windows = Window::all().map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for window in windows {
        // unwrap_or_default() を使って、失敗した場合は空文字にする
        let title = window.title().unwrap_or_default();
        let app_name = window.app_name().unwrap_or_default();
        // unwrap_or(0) で、失敗した場合はIDを0にする
        let id = window.id().unwrap_or(0);
        // タイトルが空じゃないものだけをリストに追加する
        if !title.is_empty() {
            result.push(WindowInfo {
                id,
                title,
                app_name,
            });
        }
    }
    Ok(result)
}

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
    .invoke_handler(tauri::generate_handler![get_windows])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
