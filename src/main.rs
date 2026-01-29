#![cfg_attr(target_os = "macos", allow(unexpected_cfgs))]

use eframe::egui;
use gst::prelude::*;
use std::sync::{Arc, Mutex};
use anyhow::Error;

/// WebRTC受信アプリケーションの状態を管理する構造体
struct WebRtcApp {
    pipeline: Option<gst::Pipeline>,
    logs: Arc<Mutex<Vec<String>>>,
    is_running: bool,
    video_texture: Option<egui::TextureHandle>,
    video_frame: Arc<Mutex<Option<VideoFrame>>>,
}

/// ビデオフレームデータを保持する構造体
struct VideoFrame {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Default for WebRtcApp {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        init_macos_app();

        // GStreamerの初期化
        if let Err(e) = gst::init() {
            eprintln!("Failed to initialize GStreamer: {}", e);
        }

        Self {
            pipeline: None,
            logs: Arc::new(Mutex::new(Vec::new())),
            is_running: false,
            video_texture: None,
            video_frame: Arc::new(Mutex::new(None)),
        }
    }
}

impl WebRtcApp {
    fn add_log(&self, message: String) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.push(message);
            // 最新100件のみ保持
            if logs.len() > 100 {
                logs.remove(0);
            }
        }
    }

    fn start_pipeline(&mut self) -> Result<(), Error> {
        if self.is_running {
            return Ok(());
        }

        let pipeline = gst::Pipeline::builder().build();

        // webrtcsrcの作成 - 低遅延設定
        let webrtcsrc = gst::ElementFactory::make("webrtcsrc")
            .property("connect-to-first-producer", true)
            .property_from_str("video-codecs", "<H264, VP8>")
            .property_from_str("audio-codecs", "<OPUS>")
            .property("enable-control-data-channel", true)
            .build()?;

        pipeline.add(&webrtcsrc)?;

        let signaller = webrtcsrc.property::<gst::glib::Object>("signaller");

        // ログ用のクロージャ
        let logs = self.logs.clone();
        signaller.connect("producer-added", false, move |args| {
            let producer_id = args[1].get::<String>().unwrap();
            let meta = args[2].get::<Option<gst::Structure>>().unwrap();
            if let Ok(mut logs) = logs.lock() {
                logs.push(format!("🎤 Producer追加: producer_id={}, meta={:?}", producer_id, meta));
            }
            None
        });

        let logs = self.logs.clone();
        signaller.connect("session-requested", false, move |args| {
            let session_id = args[1].get::<String>().unwrap();
            let peer_id = args[2].get::<String>().unwrap();
            if let Ok(mut logs) = logs.lock() {
                logs.push(format!("📞 セッション要求: peer_id={}, session_id={}", peer_id, session_id));
            }
            None
        });

        let logs = self.logs.clone();
        signaller.connect("session-started", false, move |args| {
            let session_id = args[1].get::<String>().unwrap();
            let peer_id = args[2].get::<String>().unwrap();
            if let Ok(mut logs) = logs.lock() {
                logs.push(format!("✅ セッション開始: peer_id={}, session_id={}", peer_id, session_id));
            }
            None
        });

        let logs = self.logs.clone();
        signaller.connect("webrtcbin-ready", false, move |args| {
            let webrtcbin = args[2].get::<gst::Element>().unwrap();
            webrtcbin.set_property("latency", 20u32);
            if let Ok(mut logs) = logs.lock() {
                logs.push("🎬 WebRTCBin ready - 低遅延設定を適用しました".to_string());
            }
            None
        });

        // pad-addedシグナル: videoとaudioのパッドを動的に接続
        let video_frame = self.video_frame.clone();
        let logs_for_pad = self.logs.clone();
        webrtcsrc.connect_pad_added(move |webrtcsrc, pad| {
            let Some(pipeline) = webrtcsrc
                .parent()
                .and_then(|p| p.downcast::<gst::Pipeline>().ok())
            else {
                return;
            };

            if pad.name().starts_with("audio") {
                if let Ok(mut logs) = logs_for_pad.lock() {
                    logs.push("🔊 Audio pad追加".to_string());
                }

                let audioconvert = gst::ElementFactory::make("audioconvert").build().unwrap();
                let audioresample = gst::ElementFactory::make("audioresample").build().unwrap();
                let queue = gst::ElementFactory::make("queue")
                    .property("max-size-buffers", 1u32)
                    .property("max-size-bytes", 0u32)
                    .property("max-size-time", 0u64)
                    .build()
                    .unwrap();
                let audiosink = gst::ElementFactory::make("autoaudiosink")
                    .build()
                    .unwrap();

                pipeline.add_many([&audioconvert, &audioresample, &queue, &audiosink]).unwrap();
                pad.link(&audioconvert.static_pad("sink").unwrap()).unwrap();
                gst::Element::link_many([&audioconvert, &audioresample, &queue, &audiosink]).unwrap();

                audiosink.sync_state_with_parent().unwrap();
                queue.sync_state_with_parent().unwrap();
                audioresample.sync_state_with_parent().unwrap();
                audioconvert.sync_state_with_parent().unwrap();
            } else if pad.name().starts_with("video") {
                if let Ok(mut logs) = logs_for_pad.lock() {
                    logs.push("🎥 Video pad追加".to_string());
                }

                let videoconvert = gst::ElementFactory::make("videoconvert").build().unwrap();
                let videoscale = gst::ElementFactory::make("videoscale").build().unwrap();
                let queue = gst::ElementFactory::make("queue")
                    .property("max-size-buffers", 1u32)
                    .property("max-size-bytes", 0u32)
                    .property("max-size-time", 0u64)
                    .build()
                    .unwrap();

                // appsinkを使用してビデオフレームをキャプチャ
                let appsink = gst_app::AppSink::builder()
                    .caps(
                        &gst::Caps::builder("video/x-raw")
                            .field("format", "RGBA")
                            .build()
                    )
                    .build();

                let video_frame_clone = video_frame.clone();
                appsink.set_callbacks(
                    gst_app::AppSinkCallbacks::builder()
                        .new_sample(move |appsink| {
                            let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                            let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                            let caps = sample.caps().ok_or(gst::FlowError::Error)?;
                            
                            let video_info = gst_video::VideoInfo::from_caps(caps)
                                .map_err(|_| gst::FlowError::Error)?;
                            
                            let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                            
                            if let Ok(mut frame) = video_frame_clone.lock() {
                                *frame = Some(VideoFrame {
                                    width: video_info.width() as usize,
                                    height: video_info.height() as usize,
                                    data: map.as_slice().to_vec(),
                                });
                            }
                            
                            Ok(gst::FlowSuccess::Ok)
                        })
                        .build()
                );

                pipeline.add_many([&videoconvert, &videoscale, &queue, appsink.upcast_ref()]).unwrap();
                pad.link(&videoconvert.static_pad("sink").unwrap()).unwrap();
                gst::Element::link_many([&videoconvert, &videoscale, &queue, appsink.upcast_ref()]).unwrap();

                appsink.sync_state_with_parent().unwrap();
                queue.sync_state_with_parent().unwrap();
                videoscale.sync_state_with_parent().unwrap();
                videoconvert.sync_state_with_parent().unwrap();
            }
        });

        // パイプライン起動
        pipeline.set_state(gst::State::Playing)?;

        // バスメッセージ処理用のスレッドを起動
        let bus = pipeline.bus().expect("Pipeline should have a bus");
        let pipeline_weak = pipeline.downgrade();
        let logs = self.logs.clone();
        
        std::thread::spawn(move || {
            for msg in bus.iter_timed(gst::ClockTime::NONE) {
                use gst::MessageView;
                
                match msg.view() {
                    MessageView::Eos(..) => {
                        if let Ok(mut logs) = logs.lock() {
                            logs.push("⏹️ EOS".to_string());
                        }
                        break;
                    }
                    MessageView::Error(err) => {
                        if let Ok(mut logs) = logs.lock() {
                            logs.push(format!("❌ Error: {}", err.error()));
                        }
                        if let Some(pipeline) = pipeline_weak.upgrade() {
                            let _ = pipeline.set_state(gst::State::Null);
                        }
                        break;
                    }
                    MessageView::Latency(_) => {
                        if let Some(pipeline) = pipeline_weak.upgrade() {
                            let _ = pipeline.recalculate_latency();
                        }
                    }
                    _ => (),
                }
            }
        });

        self.pipeline = Some(pipeline);
        self.is_running = true;
        self.add_log("▶️ パイプライン開始".to_string());

        Ok(())
    }

    fn stop_pipeline(&mut self) {
        if let Some(pipeline) = self.pipeline.take() {
            let _ = pipeline.set_state(gst::State::Null);
            self.is_running = false;
            self.add_log("⏹️ パイプライン停止".to_string());
        }
    }
}

impl eframe::App for WebRtcApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 定期的に再描画をリクエスト
        ctx.request_repaint();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("WebRTC 低遅延受信 GUI");

            ui.horizontal(|ui| {
                if ui.button(if self.is_running { "⏹️ 停止" } else { "▶️ 開始" }).clicked() {
                    if self.is_running {
                        self.stop_pipeline();
                    } else {
                        if let Err(e) = self.start_pipeline() {
                            self.add_log(format!("❌ エラー: {}", e));
                        }
                    }
                }

                ui.label(if self.is_running { "🟢 実行中" } else { "🔴 停止中" });
            });

            ui.separator();

            // ビデオ表示エリア
            ui.heading("ビデオ");
            
            if let Ok(frame_guard) = self.video_frame.lock() {
                if let Some(frame) = frame_guard.as_ref() {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.width, frame.height],
                        &frame.data,
                    );
                    
                    let texture = ctx.load_texture(
                        "video-frame",
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    
                    ui.image(&texture);
                    self.video_texture = Some(texture);
                } else {
                    ui.label("ビデオフレームを待機中...");
                }
            }

            ui.separator();

            // ログ表示エリア
            ui.heading("ログ");
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if let Ok(logs) = self.logs.lock() {
                        for log in logs.iter() {
                            ui.label(log);
                        }
                    }
                });
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_pipeline();
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("WebRTC 低遅延受信"),
        ..Default::default()
    };

    eframe::run_native(
        "webrtc-egui",
        options,
        Box::new(|cc| {
            // IBM Plex Sans JP フォントを読み込んで設定
            let mut fonts = egui::FontDefinitions::default();

            // フォントデータを追加
            fonts.font_data.insert(
                "ibm_plex_sans_jp".to_owned(),
                egui::FontData::from_static(include_bytes!("../fonts/IBMPlexSansJP-Regular.ttf")).into(),
            );

            // Proportional（プロポーショナル）フォントとして設定（優先度最高）
            fonts.families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "ibm_plex_sans_jp".to_owned());

            // Monospace（等幅）フォントとしても設定
            fonts.families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "ibm_plex_sans_jp".to_owned());

            // フォント設定を適用
            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::new(WebRtcApp::default()))
        }),
    )
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn init_macos_app() {
    use objc::{class, msg_send, sel, sel_impl};
    unsafe {
        let ns_app: *mut objc::runtime::Object = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![ns_app, finishLaunching];
    }
}
