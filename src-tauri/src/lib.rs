#[cfg_attr(mobile, tauri::mobile_entry_point)]

use serde::Serialize;
use tauri::command;
use xcap::Window;
use chrono::Local;

use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::{Frame, ImageFormat},
    graphics_capture_api::InternalCaptureControl,
    // ↓ ここに新しい設定3つ（SecondaryWindowSettings など）を追加してね
    settings::{
        ColorFormat, CursorCaptureSettings, DrawBorderSettings, Settings,
        SecondaryWindowSettings, MinimumUpdateIntervalSettings, DirtyRegionSettings
    },
    window::Window as WgcWindow,
};

// 1フレームだけ受信して保存する「受信機」
struct SnapshotHandler {
    saved: bool,
    filename: String,
}

impl GraphicsCaptureApiHandler for SnapshotHandler {
    type Flags = String;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    // 初期化の引数が Context という形に変わった
    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            saved: false,
            // ctx.flags の中に、渡したファイル名（test_wgc.png）が入っている
            filename: ctx.flags,
        })
    }

    // 映像のコマ（フレーム）が流れてくるたびに呼ばれる
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if !self.saved {
            // 最初のコマを画像として保存
            frame.save_as_image(&self.filename, ImageFormat::Png)?;
            self.saved = true;
            // 保存したら、これ以上流れてこないように録画ストリームを停止
            capture_control.stop();
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

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

#[command]
fn capture_selected_window(title: String) -> Result<String, String> {
    let handle = std::thread::spawn(move || {
        let window = WgcWindow::from_contains_name(&title).map_err(|e| e.to_string())?;

        // 現在のローカル時刻を取得して、見やすいフォーマットにする
        let now = Local::now();
        let filename = format!("Screenshot_{}.png", now.format("%Y%m%d_%H%M%S"));

        let settings = Settings::new(
            window,
            CursorCaptureSettings::Default,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            filename.clone(),
        );

        SnapshotHandler::start(settings).map_err(|e| e.to_string())?;
        Ok(format!("{} に保存したよ", filename))
    });

    match handle.join() {
        Ok(inner_result) => inner_result,
        Err(_) => Err("録画用の別スレッドがクラッシュしちゃったみたい…".to_string())
    }
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
    .invoke_handler(tauri::generate_handler![get_windows, capture_selected_window])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
