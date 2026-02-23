#[cfg_attr(mobile, tauri::mobile_entry_point)]

use serde::Serialize;
use tauri::command;
use xcap::Window;
use chrono::Local;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use cpal::traits::{DeviceTrait, HostTrait};

use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::{Frame, ImageFormat},
    graphics_capture_api::InternalCaptureControl,
    settings::{
        ColorFormat, CursorCaptureSettings, DrawBorderSettings, Settings,
        SecondaryWindowSettings, MinimumUpdateIntervalSettings, DirtyRegionSettings
    },
    window::Window as WgcWindow,
    encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder},
};

// 1フレームだけ受信して保存する
struct SnapshotHandler {
    saved: bool,
    filename: String,
}

struct VideoRecorderHandler {
    encoder: Option<VideoEncoder>,
    filename: String,
    is_recording: Arc<AtomicBool>,
}

struct RecordState {
    is_recording: Arc<AtomicBool>,
}

struct RecorderFlags {
    filename: String,
    is_recording: Arc<AtomicBool>,
}

impl GraphicsCaptureApiHandler for SnapshotHandler {
    type Flags = String;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            saved: false,
            filename: ctx.flags,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if !self.saved {
            frame.save_as_image(&self.filename, ImageFormat::Png)?;
            self.saved = true;
            capture_control.stop();
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl GraphicsCaptureApiHandler for VideoRecorderHandler {
    type Flags = RecorderFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            encoder: None,
            filename: ctx.flags.filename,
            is_recording: ctx.flags.is_recording,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.encoder.is_none() {
            let encoder = VideoEncoder::new(
                VideoSettingsBuilder::new(frame.width(), frame.height()),
                AudioSettingsBuilder::default().disabled(true), 
                ContainerSettingsBuilder::default(),
                &self.filename,
            )?;
            self.encoder = Some(encoder);
        }

        if let Some(encoder) = &mut self.encoder {
            encoder.send_frame(frame)?;
        }

        if !self.is_recording.load(Ordering::Relaxed) {
            capture_control.stop();
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        if let Some(encoder) = self.encoder.take() {
            encoder.finish()?;
        }
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
        // 失敗した場合はIDを0にする
        let id = window.id().unwrap_or(0);
        // タイトルが空じゃないものだけをリストに追加
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

// 録画開始
#[command]
fn start_record_window(title: String, state: tauri::State<'_, RecordState>) -> Result<String, String> {
    // すでに録画中なら弾く
    if state.is_recording.load(Ordering::Relaxed) {
        return Err("すでに録画中みたい…".to_string());
    }
    
    // 録画中フラグをONにする
    state.is_recording.store(true, Ordering::Relaxed);
    let is_recording_clone = Arc::clone(&state.is_recording);

    // スレッドを切り離して、裏側で勝手にやってもらう
    std::thread::spawn(move || {
        let window = match WgcWindow::from_contains_name(&title) {
            Ok(w) => w,
            Err(_) => {
                is_recording_clone.store(false, Ordering::Relaxed);
                return;
            }
        };
        let now = chrono::Local::now();
        let filename = format!("Record_{}.mp4", now.format("%Y%m%d_%H%M%S"));

        let settings = Settings::new(
            window,
            CursorCaptureSettings::Default,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            RecorderFlags {
                filename,
                is_recording: is_recording_clone.clone(),
            },
        );

        // 録画開始（止まるまでここでブロックされる）
        let _ = VideoRecorderHandler::start(settings);
        
        // 終わったらフラグをOFFに戻す
        is_recording_clone.store(false, Ordering::Relaxed);
    });

    Ok("録画を開始したよ".to_string())
}

// 録画停止
#[command]
fn stop_record_window(state: tauri::State<'_, RecordState>) -> Result<String, String> {
    state.is_recording.store(false, Ordering::Relaxed);
    Ok("録画を停止したよ".to_string())
}

#[command]
fn test_audio_device() -> Result<String, String> {
    let host = cpal::default_host();
    
    // システムの標準スピーカー（出力デバイス）を取得
    let device = host.default_output_device()
        .ok_or_else(|| "スピーカー（出力デバイス）が見つからないみたい…".to_string())?;
        
    // スピーカーの設定を取得
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    
    let device_name = device.name().unwrap_or_else(|_| "不明なデバイス".to_string());
    
    Ok(format!("音声デバイス確認OK: {} (サンプルレート: {}Hz)", device_name, config.sample_rate()))
}

pub fn run() {
  tauri::Builder::default()
    .manage(RecordState {
            is_recording: Arc::new(AtomicBool::new(false)),
        })
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
    .invoke_handler(tauri::generate_handler![get_windows, capture_selected_window, start_record_window, stop_record_window, test_audio_device])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
