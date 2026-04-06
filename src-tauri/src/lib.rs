#[cfg_attr(mobile, tauri::mobile_entry_point)]
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

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

#[derive(Serialize)]
struct WindowInfo {
    id: u32,
    title: String,
    app_name: String,
    pid: u32,
}

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



#[command]
fn get_windows() -> Result<Vec<WindowInfo>, String> {
    let windows = Window::all().map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for window in windows {
        let title = window.title().unwrap_or_default();
        let app_name = window.app_name().unwrap_or_default();
        let id = window.id().unwrap_or(0);
        if !title.is_empty() {
            // ウィンドウIDからプロセスID（PID）を取得する
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(HWND(id as _), Some(&mut pid));
            }

            result.push(WindowInfo {
                id,
                title,
                app_name,
                pid,
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
fn start_record_window(title: String,pid: u32, state: tauri::State<'_, RecordState>) -> Result<String, String> {
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

        // --- ここから書き換える ---
        let is_recording_audio = Arc::clone(&is_recording_clone);
        let audio_path_clone = audio_path.clone();
        
        let audio_thread = std::thread::spawn(move || {
            // WASAPIを使うためのおまじない（COM初期化）
            let _ = wasapi::initialize_mta().ok();

            // 1. 指定したPIDの音だけを拾うクライアントを作成
            let mut client = match wasapi::AudioClient::new_application_loopback_client(pid, true) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("プロセスループバックの作成に失敗: {:?}", e);
                    is_recording_audio.store(false, std::sync::atomic::Ordering::Relaxed);
                    return;
                }
            };

            // 2. 音の形式（フォーマット）を決める
            // 新しいバージョンでは DeviceEnumerator を使ってデフォルトデバイスを探すよ
            let enumerator = wasapi::DeviceEnumerator::new().unwrap();
            let default_device = enumerator.get_default_device(&wasapi::Direction::Render).unwrap();
            let default_client = default_device.get_iaudioclient().unwrap();
            let format = default_client.get_mixformat().unwrap();

            // 3. クライアントを初期化（イベント駆動モード）
            // 引数が整理されて、StreamMode の中に設定をまとめる形になったみたい
            let mode = wasapi::StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 200_000, // 20ms
            };
            client.initialize_client(
                &format,
                &wasapi::Direction::Capture,
                &mode,
            ).unwrap();

            let h_event = client.set_get_eventhandle().unwrap();
            let capture_client = client.get_audiocaptureclient().unwrap();
            
            client.start_stream().unwrap();

            // 4. WAVファイルを準備
            let channels = format.get_nchannels() as u16;
            let spec = hound::WavSpec {
                channels,
                sample_rate: format.get_samplespersec(),
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };
            let mut writer = hound::WavWriter::create(&audio_path_clone, spec).unwrap();

            // 新しいバージョンでは、データを受け取るための「空の箱（バッファ）」を先に用意しておく必要があるの
            let bytes_per_frame = (channels as u32) * 4; // 32ビット(4バイト) × チャンネル数
            let max_frames = client.get_buffer_size().unwrap_or(48000); // 念のため十分なサイズを確保
            let mut buffer = vec![0u8; (max_frames * bytes_per_frame) as usize];

            // 5. 録音ループ
            while is_recording_audio.load(std::sync::atomic::Ordering::Relaxed) {
                // 音声データが来るまで少し待つ
                if let Ok(_) = h_event.wait_for_event(100) {
                    // read_from_device に「空の箱」を渡して、そこに音を詰めてもらう
                    if let Ok((frames, _info)) = capture_client.read_from_device(&mut buffer) {
                        if frames > 0 {
                            let bytes_read = (frames * bytes_per_frame) as usize;
                            let read_data = &buffer[..bytes_read];

                            // 読み出したバイト列(u8)を、Float(f32)の配列に変換して書き込む
                            let float_data: &[f32] = unsafe {
                                std::slice::from_raw_parts(
                                    read_data.as_ptr() as *const f32,
                                    read_data.len() / 4,
                                )
                            };
                            for &sample in float_data {
                                let _ = writer.write_sample(sample);
                            }
                        }
                    }
                }
            }

            // 終わったらストリームを止めてファイルを閉じる
            let _ = client.stop_stream();
            let _ = writer.finalize();
        });
        // --- ここまで ---

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
        let _ = audio_thread.join(); // 音声スレッドの終了を待つ
        let _ = video_thread.join(); // 映像スレッドの終了を確実に待つ

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
