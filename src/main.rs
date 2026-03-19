#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod radio_browser;

use anyhow::Result;
use eframe::egui;
use egui::{Color32, RichText};
use radio_browser::RadioStation;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

// ── Palette ───────────────────────────────────────────────────────────────────
const BG: Color32 = Color32::from_rgb(11, 11, 20);
const BG_PANEL: Color32 = Color32::from_rgb(17, 17, 30);
const BG_CARD: Color32 = Color32::from_rgb(24, 24, 40);
const BG_PLAYING_CARD: Color32 = Color32::from_rgb(30, 15, 60);
const ACCENT: Color32 = Color32::from_rgb(124, 58, 237);
const GREEN: Color32 = Color32::from_rgb(52, 211, 153);
const RED_BTN: Color32 = Color32::from_rgb(200, 50, 50);
const TEXT: Color32 = Color32::from_rgb(226, 232, 240);
const MUTED: Color32 = Color32::from_rgb(94, 110, 130);
const BORDER: Color32 = Color32::from_rgb(36, 36, 58);
const BORDER_PLAYING: Color32 = Color32::from_rgb(80, 40, 140);
// ─────────────────────────────────────────────────────────────────────────────

fn setup_custom_style(ctx: &egui::Context) {
    let mut vis = egui::Visuals::dark();
    vis.panel_fill = BG;
    vis.window_fill = BG;
    vis.faint_bg_color = BG_PANEL;
    vis.extreme_bg_color = Color32::from_rgb(6, 6, 12);

    {
        let w = &mut vis.widgets;
        let r = egui::Rounding::same(6.0);

        w.noninteractive.bg_fill = BG_PANEL;
        w.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
        w.noninteractive.rounding = r;
        w.noninteractive.fg_stroke = egui::Stroke::new(1.0, MUTED);

        w.inactive.bg_fill = BG_CARD;
        w.inactive.weak_bg_fill = BG_CARD;
        w.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
        w.inactive.rounding = r;

        w.hovered.bg_fill = Color32::from_rgb(34, 34, 54);
        w.hovered.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(90, 70, 180));
        w.hovered.rounding = r;

        w.active.bg_fill = Color32::from_rgb(50, 40, 90);
        w.active.bg_stroke = egui::Stroke::new(1.5, ACCENT);
        w.active.rounding = r;
    }

    vis.selection.bg_fill = Color32::from_rgb(80, 40, 170);
    vis.selection.stroke = egui::Stroke::new(1.0, ACCENT);

    ctx.set_visuals(vis);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 5.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    ctx.set_style(style);
}

// ── Saved state ─────────────────────────────────────────────────────────────
#[derive(Serialize, Deserialize)]
struct SavedState {
    station: Option<RadioStation>,
    search_query: String,
}

pub enum AppAction {
    Play(RadioStation),
    Stop,
    Search(String),
}

struct RadioApp {
    search_query: String,
    stations: Vec<RadioStation>,
    current_station: Option<RadioStation>,
    is_playing: bool,

    // Audio
    _stream: Option<rodio::OutputStream>,
    stream_handle: Option<rodio::OutputStreamHandle>,
    current_sink: Option<Arc<rodio::Sink>>,

    // Metadata
    metadata_running: Arc<AtomicBool>,
    metadata_generation: Arc<AtomicU64>,
    current_metadata: Arc<Mutex<Option<String>>>,

    // Message channel for UI actions
    action_tx: mpsc::Sender<AppAction>,
    action_rx: mpsc::Receiver<AppAction>,

    // Search results from background thread
    search_results_rx: mpsc::Receiver<Vec<RadioStation>>,
    search_results_tx: mpsc::Sender<Vec<RadioStation>>,
    is_searching: bool,

    // Window state
    show_about: bool,

    // Startup / notification state
    startup_done: bool,
    last_notified_metadata: Option<String>,
}

// HTTP stream source for rodio (Read + fake Seek)
struct StreamSource {
    response: reqwest::blocking::Response,
}

impl StreamSource {
    fn new(url: &str) -> Option<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .ok()?;
        client.get(url).send().ok().map(|response| StreamSource { response })
    }
}

impl Read for StreamSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.response.read(buf)
    }
}

impl std::io::Seek for StreamSource {
    fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

impl RadioApp {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let (search_tx, search_rx) = mpsc::channel();
        let (stream, stream_handle) = rodio::OutputStream::try_default().ok().unzip();
        RadioApp {
            search_query: String::new(),
            stations: Vec::new(),
            current_station: None,
            is_playing: false,
            _stream: stream,
            stream_handle,
            current_sink: None,
            metadata_running: Arc::new(AtomicBool::new(false)),
            metadata_generation: Arc::new(AtomicU64::new(0)),
            current_metadata: Arc::new(Mutex::new(None)),
            action_tx: tx,
            action_rx: rx,
            search_results_rx: search_rx,
            search_results_tx: search_tx,
            is_searching: false,
            show_about: false,
            startup_done: false,
            last_notified_metadata: None,
        }
    }

    // ── State persistence ───────────────────────────────────────────────
    fn config_path() -> Option<std::path::PathBuf> {
        dirs::config_dir().map(|d| d.join("eradio").join("state.json"))
    }

    fn save_state(&self) {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let state = SavedState {
                station: self.current_station.clone(),
                search_query: self.search_query.clone(),
            };
            if let Ok(json) = serde_json::to_string_pretty(&state) {
                let _ = std::fs::write(path, json);
            }
        }
    }

    fn load_state() -> Option<SavedState> {
        let path = Self::config_path()?;
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    // ── Core logic ──────────────────────────────────────────────────────
    fn search(&mut self, query: &str) {
        if query.is_empty() {
            return;
        }

        self.is_searching = true;
        let url = format!(
            "https://de1.api.radio-browser.info/json/stations/byname/{}?limit=100&order=votes&reverse=true",
            urlencoding::encode(query)
        );

        let tx = self.search_results_tx.clone();
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap();

            match client
                .get(&url)
                .header("User-Agent", "RadioPlayer/1.0")
                .send()
            {
                Ok(response) => {
                    if let Ok(stations) = response.json::<Vec<RadioStation>>() {
                        let filtered: Vec<RadioStation> = stations
                            .into_iter()
                            .filter(|s| {
                                s.codec
                                    .as_ref()
                                    .map(|c| c.eq_ignore_ascii_case("MP3"))
                                    .unwrap_or(false)
                            })
                            .collect();
                        let _ = tx.send(filtered);
                    } else {
                        let _ = tx.send(Vec::new());
                    }
                }
                Err(e) => {
                    eprintln!("Search error: {}", e);
                    let _ = tx.send(Vec::new());
                }
            }
        });
    }

    fn play_station(&mut self, station: RadioStation) {
        self.stop();

        let url = station.get_stream_url();

        // Record click (background — don't block UI)
        let click_url = format!(
            "https://de1.api.radio-browser.info/json/url/{}",
            station.station_uuid
        );
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();
            let _ = client
                .get(&click_url)
                .header("User-Agent", "RadioPlayer/1.0")
                .send();
        });

        self.current_station = Some(station);
        self.is_playing = true;
        self.last_notified_metadata = None;
        self.save_state();

        self.metadata_running.store(true, Ordering::SeqCst);
        let gen = self.metadata_generation.fetch_add(1, Ordering::SeqCst) + 1;

        if let Some(ref handle) = self.stream_handle {
            if let Some(sink) = rodio::Sink::try_new(handle).ok() {
                let sink = Arc::new(sink);
                self.current_sink = Some(Arc::clone(&sink));
                let running = Arc::clone(&self.metadata_running);

                let audio_url = url.clone();
                std::thread::spawn(move || {
                    if let Some(stream) = StreamSource::new(&audio_url) {
                        if let Ok(source) = rodio::Decoder::new(stream) {
                            sink.append(source);
                            sink.play();
                            while !sink.empty() && running.load(Ordering::SeqCst) {
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            sink.stop();
                        }
                    }
                });
            }
        }

        // Metadata thread
        let meta_running = Arc::clone(&self.metadata_running);
        let meta_gen = Arc::clone(&self.metadata_generation);
        let metadata = Arc::clone(&self.current_metadata);
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap();
            let Ok(response) = client
                .get(&url)
                .header("Icy-MetaData", "1")
                .header("User-Agent", "RadioPlayer/1.0")
                .send()
            else {
                return;
            };

            let Some(meta_interval) = response
                .headers()
                .get("icy-metaint")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<usize>().ok())
            else {
                return;
            };

            let mut stream = response;
            let mut buffer = Vec::new();
            let re = regex::Regex::new(r"StreamTitle='([^']*)'").unwrap();

            while meta_running.load(Ordering::SeqCst)
                && meta_gen.load(Ordering::SeqCst) == gen
            {
                let mut chunk = vec![0u8; 8192];
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        buffer.extend_from_slice(&chunk[..n]);

                        while buffer.len() >= meta_interval + 1 {
                            buffer.drain(0..meta_interval);

                            let meta_len = buffer[0] as usize * 16;
                            let total_meta = 1 + meta_len;

                            if buffer.len() < total_meta {
                                break;
                            }

                            if meta_len > 0 {
                                if let Ok(text) =
                                    String::from_utf8(buffer[1..1 + meta_len].to_vec())
                                {
                                    if let Some(caps) = re.captures(&text) {
                                        if let Some(m) = caps.get(1) {
                                            if meta_gen.load(Ordering::SeqCst) == gen {
                                                *metadata.lock().unwrap() =
                                                    Some(m.as_str().to_string());
                                            }
                                        }
                                    }
                                }
                            }

                            buffer.drain(0..total_meta);
                        }
                    }
                    Err(_) => break,
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
    }

    fn stop(&mut self) {
        self.metadata_running.store(false, Ordering::SeqCst);
        if let Some(sink) = self.current_sink.take() {
            sink.stop();
        }
        self.is_playing = false;
        // Keep current_station so the player card still shows the last station
        *self.current_metadata.lock().unwrap() = None;
        self.last_notified_metadata = None;
    }

    fn process_actions(&mut self) {
        while let Ok(action) = self.action_rx.try_recv() {
            match action {
                AppAction::Play(station) => self.play_station(station),
                AppAction::Stop => self.stop(),
                AppAction::Search(query) => self.search(&query),
            }
        }
        // Poll for search results from background thread
        if let Ok(results) = self.search_results_rx.try_recv() {
            self.stations = results;
            self.is_searching = false;
        }
    }

    fn get_metadata(&self) -> Option<String> {
        self.current_metadata.lock().unwrap().clone()
    }
}

impl eframe::App for RadioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_actions();

        // Restore last station on first frame
        if !self.startup_done {
            self.startup_done = true;
            if let Some(saved) = Self::load_state() {
                self.search_query = saved.search_query;
                if let Some(station) = saved.station {
                    self.play_station(station);
                }
            }
        }

        let action_tx = self.action_tx.clone();

        // Pre-collect station display data (avoids borrow conflicts in closures)
        let stations: Vec<(String, String, String, i32, String)> = self
            .stations
            .iter()
            .map(|s| {
                let tags = s
                    .tags
                    .as_deref()
                    .unwrap_or("")
                    .split(',')
                    .map(|t| t.trim())
                    .filter(|t| !t.is_empty())
                    .take(2)
                    .collect::<Vec<_>>()
                    .join(" • ");
                (
                    s.name.clone(),
                    s.country.clone().unwrap_or_default(),
                    s.station_uuid.clone(),
                    s.bitrate,
                    tags,
                )
            })
            .collect();

        let current_uuid = self
            .current_station
            .as_ref()
            .map(|s| s.station_uuid.clone());
        let current_name = self.current_station.as_ref().map(|s| s.name.clone());
        let is_playing = self.is_playing;
        let metadata = self.get_metadata();
        let station_count = self.stations.len();
        let time = ctx.input(|i| i.time);

        let panel_frame = egui::Frame::none()
            .fill(BG)
            .inner_margin(egui::Margin::same(12.0));

        egui::CentralPanel::default()
            .frame(panel_frame)
            .show(ctx, |ui| {
                // ── Player card (always visible) ────────────────────────
                let has_station = current_name.is_some();
                egui::Frame::none()
                        .fill(BG_PLAYING_CARD)
                        .rounding(12.0)
                        .stroke(egui::Stroke::new(1.0, BORDER_PLAYING))
                        .inner_margin(egui::Margin::same(16.0))
                        .show(ui, |ui: &mut egui::Ui| {
                            if !has_station {
                                // Idle state
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("No station playing")
                                            .size(14.0)
                                            .color(MUTED),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("About")
                                                            .color(TEXT)
                                                            .size(11.0),
                                                    )
                                                    .fill(BG_CARD)
                                                    .rounding(4.0),
                                                )
                                                .clicked()
                                            {
                                                self.show_about = true;
                                            }
                                        },
                                    );
                                });
                            } else {
                                // Row 1: Station name
                                if let Some(ref name) = current_name {
                                    ui.label(
                                        RichText::new(name)
                                            .size(16.0)
                                            .strong()
                                            .color(TEXT),
                                    );
                                }

                                // Scrolling animated color metadata (yellow/orange flowing)
                                if let Some(ref track) = metadata {
                                    if !track.is_empty() {
                                        ui.add_space(6.0);

                                        let avail_w = ui.available_width();
                                        let font_id = egui::FontId::proportional(17.0);

                                        let full_galley = ui.painter().layout_no_wrap(
                                            track.clone(), font_id.clone(), Color32::WHITE,
                                        );
                                        let text_w = full_galley.size().x;
                                        let line_h = full_galley.size().y + 4.0;

                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::vec2(avail_w, line_h),
                                            egui::Sense::hover(),
                                        );

                                        let painter = ui.painter().with_clip_rect(rect);
                                        let t = time as f32;

                                        // If text is wider than available space, scroll it
                                        let gap = 50.0; // small gap between repeated text
                                        let scroll_offset = if text_w > avail_w {
                                            let total = text_w + gap;
                                            let scroll_speed = 80.0; // pixels per second
                                            let phase = (t * scroll_speed) % total;
                                            -phase
                                        } else {
                                            0.0
                                        };

                                        // Draw the text, then draw it again immediately after for seamless looping
                                        let chars: Vec<char> = track.chars().collect();
                                        let repeat_count = if text_w > avail_w { 2 } else { 1 };
                                        for rep in 0..repeat_count {
                                        let rep_offset = rep as f32 * (text_w + gap);
                                        let mut x_pos = rect.left() + scroll_offset + rep_offset;

                                        for (i, ch) in chars.iter().enumerate() {
                                            // Wave between yellow and orange
                                            let wave = ((t * 2.0 - i as f32 * 0.2).sin() * 0.5 + 0.5)
                                                .clamp(0.0, 1.0);
                                            // yellow (255,220,0) <-> orange (255,130,0)
                                            let g = (220.0 * (1.0 - wave) + 130.0 * wave) as u8;
                                            let color = Color32::from_rgb(255, g, 0);

                                            let galley = painter.layout_no_wrap(
                                                ch.to_string(), font_id.clone(), color,
                                            );
                                            let char_w = galley.size().x;

                                            // Only paint if visible
                                            if x_pos + char_w > rect.left() && x_pos < rect.right() {
                                                painter.galley(
                                                    egui::pos2(x_pos, rect.top() + 2.0),
                                                    galley,
                                                    Color32::TRANSPARENT,
                                                );
                                            }
                                            x_pos += char_w;
                                        }
                                        }
                                    }
                                }

                                // Bottom row: About button (left) + Stop/Play button (right)
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("About")
                                                    .color(TEXT)
                                                    .size(11.0),
                                            )
                                            .fill(BG_CARD)
                                            .rounding(4.0),
                                        )
                                        .clicked()
                                    {
                                        self.show_about = true;
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if is_playing {
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new("Stop")
                                                                .color(Color32::WHITE)
                                                                .size(12.0),
                                                        )
                                                        .fill(RED_BTN)
                                                        .rounding(6.0)
                                                        .min_size(egui::vec2(64.0, 26.0)),
                                                    )
                                                    .clicked()
                                                {
                                                    let _ = action_tx.send(AppAction::Stop);
                                                }
                                            } else {
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new("Play")
                                                                .color(Color32::WHITE)
                                                                .size(12.0),
                                                        )
                                                        .fill(ACCENT)
                                                        .rounding(6.0)
                                                        .min_size(egui::vec2(64.0, 26.0)),
                                                    )
                                                    .clicked()
                                                {
                                                    if let Some(ref station) = self.current_station {
                                                        let _ = action_tx
                                                            .send(AppAction::Play(station.clone()));
                                                    }
                                                }
                                            }
                                        },
                                    );
                                });
                            }
                        });

                ui.add_space(10.0);

                // ── Search bar ──────────────────────────────────────────
                let mut do_search = false;
                egui::Frame::none()
                    .fill(BG_CARD)
                    .rounding(8.0)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .inner_margin(egui::Margin {
                        left: 10.0,
                        right: 6.0,
                        top: 6.0,
                        bottom: 6.0,
                    })
                    .show(ui, |ui: &mut egui::Ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("🔍").size(14.0).color(MUTED));
                            let text_width = (ui.available_width() - 90.0).max(80.0);
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.search_query)
                                    .desired_width(text_width)
                                    .hint_text("Search stations by name…")
                                    .frame(false),
                            );
                            if resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                do_search = true;
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Search")
                                            .color(Color32::WHITE)
                                            .size(13.0),
                                    )
                                    .fill(ACCENT)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(70.0, 28.0)),
                                )
                                .clicked()
                            {
                                do_search = true;
                            }
                        });
                    });

                if do_search {
                    let _ =
                        action_tx.send(AppAction::Search(self.search_query.clone()));
                }

                ui.add_space(10.0);

                // ── Empty state ─────────────────────────────────────────
                if station_count == 0 {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("🎙").size(42.0));
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("Search to discover stations")
                                .size(14.0)
                                .color(MUTED),
                        );
                        ui.label(
                            RichText::new(
                                "Type a genre, city, or station name above",
                            )
                            .size(11.0)
                            .color(Color32::from_rgb(65, 78, 95)),
                        );
                    });
                    return;
                }

                // ── Station count ───────────────────────────────────────
                ui.label(
                    RichText::new(format!("{} stations", station_count))
                        .size(11.0)
                        .color(MUTED),
                );
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);

                // ── Station list ────────────────────────────────────────
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (i, (name, country, uuid, bitrate, tags)) in
                            stations.iter().enumerate()
                        {
                            let is_current =
                                current_uuid.as_deref() == Some(uuid.as_str());
                            let atx = action_tx.clone();

                            ui.push_id(i, |ui| {
                                let frame_fill = if is_current {
                                    BG_PLAYING_CARD
                                } else {
                                    BG_CARD
                                };
                                let frame_stroke = if is_current {
                                    egui::Stroke::new(1.0, BORDER_PLAYING)
                                } else {
                                    egui::Stroke::new(1.0, BORDER)
                                };

                                egui::Frame::none()
                                    .fill(frame_fill)
                                    .rounding(8.0)
                                    .stroke(frame_stroke)
                                    .inner_margin(egui::Margin {
                                        left: 8.0,
                                        right: 12.0,
                                        top: 8.0,
                                        bottom: 8.0,
                                    })
                                    .show(ui, |ui: &mut egui::Ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            let btn_label = if is_current {
                                                "⏸"
                                            } else {
                                                "▶"
                                            };
                                            let btn_fill = if is_current {
                                                GREEN
                                            } else {
                                                ACCENT
                                            };
                                            let btn = ui.add(
                                                egui::Button::new(
                                                    RichText::new(btn_label)
                                                        .color(Color32::WHITE)
                                                        .size(12.0),
                                                )
                                                .fill(btn_fill)
                                                .rounding(20.0)
                                                .min_size(egui::vec2(32.0, 32.0)),
                                            );
                                            if btn.clicked() {
                                                if is_current {
                                                    let _ =
                                                        atx.send(AppAction::Stop);
                                                } else if let Some(station) = self
                                                    .stations
                                                    .iter()
                                                    .find(|s| {
                                                        &s.station_uuid == uuid
                                                    })
                                                    .cloned()
                                                {
                                                    let _ = atx.send(
                                                        AppAction::Play(station),
                                                    );
                                                }
                                            }

                                            ui.add_space(8.0);

                                            ui.vertical(|ui| {
                                                ui.add_space(2.0);
                                                ui.label(
                                                    RichText::new(name.as_str())
                                                        .size(13.0)
                                                        .strong()
                                                        .color(TEXT),
                                                );
                                                let mut parts: Vec<String> = vec![];
                                                if !country.is_empty() {
                                                    parts.push(country.clone());
                                                }
                                                if !tags.is_empty() {
                                                    parts.push(tags.clone());
                                                }
                                                if *bitrate > 0 {
                                                    parts.push(format!(
                                                        "{} kbps",
                                                        bitrate
                                                    ));
                                                }
                                                let info = parts.join("  •  ");
                                                if !info.is_empty() {
                                                    ui.label(
                                                        RichText::new(info)
                                                            .size(11.0)
                                                            .color(MUTED),
                                                    );
                                                }
                                            });
                                        });
                                    });
                            });

                            ui.add_space(4.0);
                        }
                    });
            });

        // ── About dialog ────────────────────────────────────────────────
        if self.show_about {
            egui::Window::new("About eRadio")
                .open(&mut self.show_about)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new("📻").size(48.0));
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("eRadio")
                                .size(20.0)
                                .strong()
                                .color(TEXT),
                        );
                        ui.label(
                            RichText::new("v0.1.0").size(12.0).color(MUTED),
                        );
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("Internet radio stream player")
                                .size(13.0)
                                .color(TEXT),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Developed by Ted Lazaros")
                                .size(12.0)
                                .color(TEXT),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Powered by radio-browser.info")
                                .size(11.0)
                                .color(MUTED),
                        );
                        ui.add_space(8.0);
                    });
                });
        }

        // ── Desktop notification on song change ─────────────────────────
        if is_playing {
            if let Some(ref meta) = metadata {
                if !meta.is_empty()
                    && self.last_notified_metadata.as_ref() != Some(meta)
                {
                    self.last_notified_metadata = Some(meta.clone());
                    let body = meta.clone();
                    let summary = current_name
                        .clone()
                        .unwrap_or_else(|| "eRadio".to_string());
                    std::thread::spawn(move || {
                        let _ = notify_rust::Notification::new()
                            .appname("eRadio")
                            .summary(&summary)
                            .body(&body)
                            .icon("audio-speakers")
                            .show();
                    });
                }
            }
        } else {
            self.last_notified_metadata = None;
        }

        // Always request a repaint so the event loop never sleeps indefinitely.
        // This prevents the window manager from thinking the app is unresponsive.
        // When playing, repaint fast for animations; otherwise use a slow idle tick.
        if is_playing || self.is_searching {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}

fn load_icon() -> Option<egui::IconData> {
    let png_bytes = include_bytes!("../eradio.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes.as_slice()));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());

    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity((info.width * info.height * 4) as usize);
            for chunk in buf.chunks(3) {
                rgba.extend_from_slice(chunk);
                rgba.push(255);
            }
            rgba
        }
        _ => return None,
    };

    Some(egui::IconData {
        rgba,
        width: info.width,
        height: info.height,
    })
}

fn main() -> Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([440.0, 660.0])
        .with_title("eRadio")
        .with_always_on_top();

    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "eRadio",
        options,
        Box::new(|cc| {
            setup_custom_style(&cc.egui_ctx);
            Ok(Box::new(RadioApp::new()))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(())
}
