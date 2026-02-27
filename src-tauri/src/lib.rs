#[cfg_attr(mobile, tauri::mobile_entry_point)]

use serde::Serialize;
use tauri::command;
use xcap::Window;
use chrono::Local;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, Mutex};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{SampleFormat, WavSpec};
use std::time::Duration;

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
        // 保存先を一時フォルダ（Temp）にしてホットリロードを回避
        let temp_dir = std::env::temp_dir();
        let video_path = temp_dir.join("temp_video.mp4");
        let audio_path = temp_dir.join("temp_audio.wav");

        // 保存先
        let now = chrono::Local::now();
        let final_filename = format!("Record_{}.mp4", now.format("%Y%m%d_%H%M%S"));
        let final_dir = dirs::video_dir().unwrap_or(temp_dir.clone());
        let final_path = final_dir.join(final_filename);

        // 音声
        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(d) => d,
            None => {
                is_recording_clone.store(false, Ordering::Relaxed);
                return;
            }
        };
        let config = device.default_output_config().unwrap();
        let spec = hound::WavSpec {
            channels: config.channels(),
            sample_rate: config.sample_rate(),
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        
        let writer = hound::WavWriter::create(&audio_path, spec).unwrap();
        let writer = Arc::new(Mutex::new(writer));
        let writer_clone = Arc::clone(&writer);

        let stream_config: cpal::StreamConfig = config.into();
        let stream = device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &_| {
                if let Ok(mut w) = writer_clone.lock() {
                    for &sample in data {
                        let _ = w.write_sample(sample);
                    }
                }
            },
            |err| eprintln!("音声エラー: {}", err),
            None,
        ).unwrap();
        stream.play().unwrap();

        // 映像
        let is_recording_video = Arc::clone(&is_recording_clone);
        let video_path_str = video_path.to_string_lossy().to_string();
        let title_clone = title.clone();
        
        let video_thread = std::thread::spawn(move || {
            let window = match WgcWindow::from_contains_name(&title_clone) {
                Ok(w) => w,
                Err(_) => {
                    is_recording_video.store(false, Ordering::Relaxed);
                    return;
                }
            };

            let settings = Settings::new(
                window,
                CursorCaptureSettings::Default,
                DrawBorderSettings::WithoutBorder,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Rgba8,
                RecorderFlags {
                    filename: video_path_str,
                    is_recording: is_recording_video,
                },
            );

            let _ = VideoRecorderHandler::start(settings);
        });

        // 待機ループ　FEから停止されるまで
        while is_recording_clone.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // 録画終了
        drop(stream);
        drop(writer);

        let _ = video_thread.join(); // 映像の終了を確実に待つ

        // 保存
        let _ = std::process::Command::new("ffmpeg")
            .arg("-y") // 上書き許可
            .arg("-i").arg(&video_path)
            .arg("-i").arg(&audio_path)
            .arg("-c:v").arg("copy")
            .arg("-c:a").arg("aac")
            .arg(&final_path)
            .output(); // コマンド実行！

        // 使い終わった一時ファイルを削除
        let _ = std::fs::remove_file(video_path);
        let _ = std::fs::remove_file(audio_path);
        
        println!("録画完了: {:?}", final_path);
    });

    Ok("録画を開始したよ。終わったらPCの「ビデオ」フォルダを確認してみて。".to_string())
}

// 録画停止
#[command]
fn stop_record_window(state: tauri::State<'_, RecordState>) -> Result<String, String> {
    state.is_recording.store(false, Ordering::Relaxed);
    Ok("録画を停止したよ".to_string())
}

#[command]
fn test_audio_record() -> Result<String, String> {
    let host = cpal::default_host();
    // PCのメインスピーカーを取得
    let device = host.default_output_device().ok_or("スピーカーが見つからないみたい")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;

    // WAVファイルの設定（スピーカーの設定に合わせる）
    let spec = WavSpec {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };

    // 保存するファイルを作成
    let writer = hound::WavWriter::create("test_audio.wav", spec).map_err(|e| e.to_string())?;
    // 裏側のスレッド（録音部屋）に渡すために Arc<Mutex> で包む
    let writer = Arc::new(Mutex::new(writer));
    let writer_clone = Arc::clone(&writer);

    let stream_config = config.clone().into();

    // 出力デバイス（スピーカー）の音を拾う「ループバック録音」を開始
    let stream = device.build_input_stream(
        &stream_config,
        move |data: &[f32], _: &_| {
            // 音の波形データが流れてくるたびに、WAVファイルに書き込む
            if let Ok(mut w) = writer_clone.lock() {
                for &sample in data {
                    let _ = w.write_sample(sample);
                }
            }
        },
        |err| eprintln!("音声エラー: {}", err),
        None,
    ).map_err(|e| e.to_string())?;

    stream.play().map_err(|e| e.to_string())?;

    // 【テスト用】5秒間待機（この間にPCから鳴っている音が録音される）
    std::thread::sleep(Duration::from_secs(5));

    // 5秒経ったら録音ストリームを閉じる
    drop(stream);
    
    // 最後にファイルの「フタ」を確実に閉める
    if let Ok(mut w) = writer.lock() {
        let _ = w.flush();
    }

    Ok("test_audio.wav に5秒間の音声を保存したよ".to_string())
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
    .invoke_handler(tauri::generate_handler![get_windows, capture_selected_window, start_record_window, stop_record_window, test_audio_record])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
