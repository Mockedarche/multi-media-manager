/*
 * Multi Media Manager - A simple rust EGUI wrapper for now FFMPEG but in the future tesseract, OCR etc
 * Goal is simple TAKE whatever media and change it to whatever codecs
 */

use eframe::egui;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};

// Communication for the thread (so we know the status of the task)
enum ProgressMsg {
    Started(usize),
    Finished(usize),
    Failed(usize, String),
    AllDone,
}

// Task status enum
#[derive(PartialEq, Clone)]
enum FileStatus {
    Idle,
    Processing,
    Done,
    Failed,
}

// Set viewpoirt dimensions, and then run the actual GUI with our desired style
fn main() -> eframe::Result<()> {
    let mut options = eframe::NativeOptions::default();
    options.viewport = egui::ViewportBuilder::default().with_inner_size([1460.0, 750.0]);

    eframe::run_native(
        "Multi Media Master",
        options,
        Box::new(|cc| {
            let mut style = (*cc.egui_ctx.style()).clone();
            style.text_styles.insert(
                egui::TextStyle::Body,
                egui::FontId::new(16.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Button,
                egui::FontId::new(16.0, egui::FontFamily::Proportional),
            );
            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            cc.egui_ctx.set_style(style);
            Box::new(MyApp::new())
        }),
    )
}

// Variables for ffmpeg codec selection
const VIDEO_CODECS: &[&str] = &[
    "copy",
    "libx264",
    "libx265",
    "libvpx-vp9",
    "libaom-av1",
    "mpeg4",
];
const AUDIO_CODECS: &[&str] = &["copy", "aac", "libmp3lame", "libopus", "flac", "pcm_s16le"];
const SUB_CODECS: &[&str] = &["copy", "mov_text", "srt", "ass", "webvtt"];

/*
 * friendly_name - Function
 * Expects: NA
 * Does: Takes FFMPEGs codec name and makes it a user friendly version
 * Returns: A string name for a codec (in a user friendly version)
 */
fn friendly_name(codec: &str) -> &str {
    match codec {
        "libx264" => "x264",
        "libx265" => "x265",
        "libvpx-vp9" => "VP9",
        "libaom-av1" => "AV1",
        "libmp3lame" => "MP3",
        "libopus" => "Opus",
        "pcm_s16le" => "WAV",
        "mov_text" => "MP4 Sub",
        _ => codec,
    }
}

/*
 * MediaFile - Struct
 * Used to hold all the data related to a file entry
 */
struct MediaFile {
    full_path: PathBuf,
    file_name: String,
    video_codec: String,
    audio_codec: String,
    subtitle_type: String,
    new_video_codec: String,
    new_audio_codec: String,
    new_subtitle_type: String,
    status: FileStatus,
}

/*
 * MyApp - Struct
 * Holds data for the file, CRF, audio_bitrate, etc the data needed to have a wrapper for FFMPEG
 */
struct MyApp {
    files: Vec<MediaFile>,
    crf: i32,
    audio_bitrate: String,
    threads: i32,
    replace_original: bool,
    default_v: String,
    default_a: String,
    default_s: String,
    tx: Sender<ProgressMsg>,
    rx: Receiver<ProgressMsg>,
    is_converting: bool,
}

/*
 * MyApp - Implementation
 * Implements all the data in MyApp struct and functions used by the GUI's buttons
 */
impl MyApp {
    fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            files: Vec::new(),
            crf: 23,
            audio_bitrate: "192k".to_string(),
            threads: 0,
            replace_original: false,
            default_v: "copy".to_string(),
            default_a: "copy".to_string(),
            default_s: "copy".to_string(),
            tx,
            rx,
            is_converting: false,
        }
    }
    /*
     * add_single_file - Function
     * Expects: Self to be intialized
     * Does: Takes the file at the path and if its a media file adds
     * Returns: NA
     */
    fn add_single_file(&mut self, path: PathBuf) {
        let path_str = path.display().to_string();
        let (v, a, s) = get_media_info(&path_str);
        if v != "Invalid" && (v != "None" || a != "None" || s != "None") {
            self.files.push(MediaFile {
                full_path: path.clone(),
                file_name: path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string(),
                video_codec: v.clone(),
                audio_codec: a.clone(),
                subtitle_type: s.clone(),
                new_video_codec: if v == "None" {
                    "None".into()
                } else {
                    self.default_v.clone()
                },
                new_audio_codec: if a == "None" {
                    "None".into()
                } else {
                    self.default_a.clone()
                },
                new_subtitle_type: if s == "None" {
                    "None".into()
                } else {
                    self.default_s.clone()
                },
                status: FileStatus::Idle,
            });
        }
    }
}

/*
 * App - implementation
 * The entire UI is layed out and logic is connected here. Goal is just UI not actual logic to perform actions
 */
impl eframe::App for MyApp {
    /* update - eframe specific Function
     * Expects: self to be intialized
     * Does: Performs the update loop of seeing if a task started, finsihed, failed, or ALLDone and
     * then it performs a draw of the GUI
     * Returns: NA
     */
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // gets message from task
        while let Ok(msg) = self.rx.try_recv() {
            // Updates GUI based on task progress
            match msg {
                ProgressMsg::Started(i) => self.files[i].status = FileStatus::Processing,
                ProgressMsg::Finished(i) => self.files[i].status = FileStatus::Done,
                ProgressMsg::Failed(i, log) => {
                    self.files[i].status = FileStatus::Failed;
                    eprintln!(
                        "--- FFMPEG FAILURE LOG ---\n{}\n--------------------------",
                        log
                    );
                }
                ProgressMsg::AllDone => self.is_converting = false,
            }
            // redraw GUI
            ctx.request_repaint();
        }

        // Side panel holds static variables for all tasks and buttons
        egui::SidePanel::left("side_panel")
            .resizable(false)
            .default_width(160.0)
            // holds all the buttons and input boxes
            .show(ctx, |ui| {
                ui.add_space(20.0);
                let button_size = egui::vec2(120.0, 40.0);
                // we only allow editing when we aren't currently doing a task
                ui.set_enabled(!self.is_converting);

                // Add file, Add Folder buttons, and bitrate, thread, replace OG, and CRF input
                ui.vertical_centered(|ui| {
                    if ui
                        .add_sized(button_size, egui::Button::new("Add File"))
                        .clicked()
                    {
                        if let Some(paths) = rfd::FileDialog::new().pick_files() {
                            for path in paths {
                                self.add_single_file(path);
                            }
                        }
                    }
                    ui.add_space(10.0);

                    if ui
                        .add_sized(button_size, egui::Button::new("Add Folder"))
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            let mut found_files = Vec::new();
                            collect_files_recursive(&path, &mut found_files);
                            for p in found_files {
                                self.add_single_file(p);
                            }
                        }
                    }

                    ui.add_space(20.0);
                    ui.separator();
                    // Constant rate factor input
                    ui.label(egui::RichText::new("Video Quality (CRF)").strong());
                    ui.add(egui::DragValue::new(&mut self.crf).clamp_range(0..=51));
                    ui.add_space(15.0);
                    // Audio bitrate input
                    ui.label(egui::RichText::new("Audio Bitrate").strong());
                    ui.text_edit_singleline(&mut self.audio_bitrate);
                    ui.add_space(15.0);
                    // cpu thread input
                    ui.label(egui::RichText::new("CPU Threads").strong());
                    ui.add(egui::DragValue::new(&mut self.threads).clamp_range(0..=64));
                    // replace original check box
                    ui.add_space(10.0);
                    ui.checkbox(&mut self.replace_original, "Replace Original");
                });

                // Convert button more complicated since it dispatches the task
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(20.0);
                    ui.set_enabled(true);
                    if ui
                        .add_sized(button_size, egui::Button::new("Convert"))
                        .clicked()
                        && !self.is_converting
                    {
                        self.is_converting = true;
                        let tx = self.tx.clone();
                        let ctx_thread = ctx.clone();
                        let audio_br = self.audio_bitrate.clone();
                        let do_replace = self.replace_original;

                        let tasks: Vec<(
                            usize,
                            PathBuf,
                            String,
                            String,
                            String,
                            String,
                            String,
                            String,
                        )> = self
                            .files
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| {
                                f.status == FileStatus::Idle || f.status == FileStatus::Failed
                            })
                            .map(|(i, f)| {
                                (
                                    i,
                                    f.full_path.clone(),
                                    f.new_video_codec.clone(),
                                    f.new_audio_codec.clone(),
                                    f.new_subtitle_type.clone(),
                                    f.file_name.clone(),
                                    self.crf.to_string(),
                                    self.threads.to_string(),
                                )
                            })
                            .collect();
                        // spawn a thread for the task making sure we keep the thread updated
                        std::thread::spawn(move || {
                            for (idx, path, v_codec, a_codec, s_codec, name, crf_val, threads) in
                                tasks
                            {
                                let _ = tx.send(ProgressMsg::Started(idx));
                                ctx_thread.request_repaint();

                                // Use a unique temp name during processing
                                let temp_out =
                                    path.with_file_name(format!("{}_tmp_proc.mkv", name));

                                // build arguments
                                let mut args = vec![
                                    "-i".to_string(),
                                    path.to_str().unwrap().into(),
                                    "-map".into(),
                                    "0".into(),
                                    "-threads".into(),
                                    threads,
                                ];

                                // we build the video codec section for the given task
                                args.extend(["-c:v".into(), v_codec.clone()]);
                                if v_codec != "copy" && v_codec != "None" {
                                    args.extend([
                                        "-crf".into(),
                                        crf_val,
                                        "-vf".into(),
                                        "scale=trunc(iw/2)*2:trunc(ih/2)*2".into(),
                                    ]);
                                }
                                // we build the audio codec section for the given task
                                args.extend(["-c:a".into(), a_codec.clone()]);
                                if a_codec != "copy" && a_codec != "None" {
                                    args.extend(["-b:a".into(), audio_br.clone()]);
                                    args.extend(["-ac".into(), "6".into()]);
                                    args.extend([
                                        "-af".into(),
                                        "aresample=async=1,aformat=channel_layouts=5.1".into(),
                                    ]);
                                    if a_codec == "libopus" {
                                        args.extend(["-mapping_family".into(), "1".into()]);
                                    }
                                }

                                // we build the subtitle codec section for the given task
                                if s_codec == "None" {
                                    args.extend(["-sn".into()]);
                                } else {
                                    args.extend(["-c:s".into(), s_codec]);
                                }

                                // build the command and launch it
                                let output = Command::new("ffmpeg")
                                    .args(&args)
                                    .arg("-y")
                                    .arg(&temp_out)
                                    .output();

                                // if output (the ffmpeg exit code)
                                match output {
                                    // if the task was successfull
                                    Ok(out) if out.status.success() => {
                                        if do_replace {
                                            let _ = std::fs::remove_file(&path);
                                            let _ = std::fs::rename(&temp_out, &path);
                                        } else {
                                            let final_out =
                                                path.with_file_name(format!("{}_new.mkv", name));
                                            let _ = std::fs::rename(&temp_out, final_out);
                                        }
                                        let _ = tx.send(ProgressMsg::Finished(idx));
                                    }
                                    // if the task failed
                                    Ok(out) => {
                                        let log = format!(
                                            "STDOUT:\n{}\nSTDERR:\n{}",
                                            String::from_utf8_lossy(&out.stdout),
                                            String::from_utf8_lossy(&out.stderr)
                                        );
                                        let _ = tx.send(ProgressMsg::Failed(idx, log));
                                        let _ = std::fs::remove_file(&temp_out);
                                    }
                                    // if the task actually errored out
                                    Err(e) => {
                                        let _ = tx.send(ProgressMsg::Failed(idx, e.to_string()));
                                        let _ = std::fs::remove_file(&temp_out);
                                    }
                                }
                                // redraw the gui
                                ctx_thread.request_repaint();
                            }
                            // send AllDone message to main thread
                            let _ = tx.send(ProgressMsg::AllDone);
                            ctx_thread.request_repaint();
                        });
                    }
                    // if we have files added add a Clear All button that would clear out the entire stack
                    ui.add_space(8.0);
                    if !self.files.is_empty()
                        && ui
                            .add_sized(button_size, egui::Button::new("Clear All"))
                            .clicked()
                    {
                        self.files.clear();
                    }
                });
            });

        // GUI area for all the tasks
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let codec_w = 115.0;
                let spacing_h = 12.0;
                let status_w = 80.0;
                let name_w =
                    (ui.available_width() - (7.0 * (codec_w + spacing_h)) - status_w - 30.0)
                        .max(300.0);

                egui::Grid::new("media_grid")
                    .num_columns(9)
                    .spacing([spacing_h, 15.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.add_sized(
                            [name_w, 20.0],
                            egui::Label::new(egui::RichText::new("File Name").strong()),
                        );
                        ui.add_sized(
                            [codec_w, 20.0],
                            egui::Label::new(egui::RichText::new("Src Video").strong()),
                        );
                        ui.add_sized(
                            [codec_w, 20.0],
                            egui::Label::new(egui::RichText::new("Src Audio").strong()),
                        );
                        ui.add_sized(
                            [codec_w, 20.0],
                            egui::Label::new(egui::RichText::new("Src Sub").strong()),
                        );
                        ui.add_sized(
                            [codec_w, 20.0],
                            egui::Label::new(egui::RichText::new("Target Video").strong()),
                        );
                        ui.add_sized(
                            [codec_w, 20.0],
                            egui::Label::new(egui::RichText::new("Target Audio").strong()),
                        );
                        ui.add_sized(
                            [codec_w, 20.0],
                            egui::Label::new(egui::RichText::new("Target Sub").strong()),
                        );
                        ui.allocate_ui(egui::vec2(codec_w, 20.0), |ui| {
                            ui.vertical_centered(|ui| ui.strong("Action"));
                        });
                        ui.add_sized(
                            [status_w, 20.0],
                            egui::Label::new(egui::RichText::new("Status").strong()),
                        );
                        ui.end_row();

                        ui.add_sized(
                            [name_w, 25.0],
                            egui::Label::new(
                                egui::RichText::new("SET ALL ->")
                                    .color(egui::Color32::LIGHT_BLUE)
                                    .strong(),
                            ),
                        );
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        // drop down for desired video codec
                        let v_changed = egui::ComboBox::from_id_source("global_v")
                            .width(codec_w)
                            .selected_text(friendly_name(&self.default_v))
                            .show_ui(ui, |ui| {
                                let mut changed = false;
                                for c in VIDEO_CODECS {
                                    if ui
                                        .selectable_value(
                                            &mut self.default_v,
                                            c.to_string(),
                                            friendly_name(c),
                                        )
                                        .clicked()
                                    {
                                        changed = true;
                                    }
                                }
                                changed
                            })
                            .inner
                            .unwrap_or(false);
                        // drop down for desired audio codec
                        let a_changed = egui::ComboBox::from_id_source("global_a")
                            .width(codec_w)
                            .selected_text(friendly_name(&self.default_a))
                            .show_ui(ui, |ui| {
                                let mut changed = false;
                                for c in AUDIO_CODECS {
                                    if ui
                                        .selectable_value(
                                            &mut self.default_a,
                                            c.to_string(),
                                            friendly_name(c),
                                        )
                                        .clicked()
                                    {
                                        changed = true;
                                    }
                                }
                                changed
                            })
                            .inner
                            .unwrap_or(false);
                        // drop down for desired subtitle codec
                        let s_changed = egui::ComboBox::from_id_source("global_s")
                            .width(codec_w)
                            .selected_text(friendly_name(&self.default_s))
                            .show_ui(ui, |ui| {
                                let mut changed = false;
                                for c in SUB_CODECS {
                                    if ui
                                        .selectable_value(
                                            &mut self.default_s,
                                            c.to_string(),
                                            friendly_name(c),
                                        )
                                        .clicked()
                                    {
                                        changed = true;
                                    }
                                }
                                changed
                            })
                            .inner
                            .unwrap_or(false);
                        // if the codec type has been selected as something different
                        if v_changed {
                            for f in self.files.iter_mut() {
                                if f.video_codec != "None" {
                                    f.new_video_codec = self.default_v.clone();
                                }
                            }
                        }
                        if a_changed {
                            for f in self.files.iter_mut() {
                                if f.audio_codec != "None" {
                                    f.new_audio_codec = self.default_a.clone();
                                }
                            }
                        }
                        if s_changed {
                            for f in self.files.iter_mut() {
                                if f.subtitle_type != "None" {
                                    f.new_subtitle_type = self.default_s.clone();
                                }
                            }
                        }
                        ui.label("");
                        ui.label("");
                        ui.end_row();

                        let mut to_remove = None;
                        for (i, file) in self.files.iter_mut().enumerate() {
                            ui.add_sized(
                                [name_w, 20.0],
                                egui::Label::new(&file.file_name).truncate(true),
                            );
                            ui.add_sized(
                                [codec_w, 20.0],
                                egui::Label::new(friendly_name(&file.video_codec)),
                            );
                            ui.add_sized(
                                [codec_w, 20.0],
                                egui::Label::new(friendly_name(&file.audio_codec)),
                            );
                            ui.add_sized(
                                [codec_w, 20.0],
                                egui::Label::new(friendly_name(&file.subtitle_type)),
                            );
                            for (codec_type, current, list) in [
                                ("v", &mut file.new_video_codec, VIDEO_CODECS),
                                ("a", &mut file.new_audio_codec, AUDIO_CODECS),
                                ("s", &mut file.new_subtitle_type, SUB_CODECS),
                            ] {
                                let original = match codec_type {
                                    "v" => &file.video_codec,
                                    "a" => &file.audio_codec,
                                    _ => &file.subtitle_type,
                                };
                                if original == "None" {
                                    ui.add_sized([codec_w, 20.0], egui::Label::new("None"));
                                } else {
                                    ui.set_enabled(!self.is_converting);
                                    egui::ComboBox::from_id_source(format!("{}_{}", codec_type, i))
                                        .width(codec_w)
                                        .selected_text(friendly_name(current))
                                        .show_ui(ui, |ui| {
                                            for c in list {
                                                ui.selectable_value(
                                                    current,
                                                    c.to_string(),
                                                    friendly_name(c),
                                                );
                                            }
                                        });
                                }
                            }
                            ui.allocate_ui(egui::vec2(codec_w, 25.0), |ui| {
                                ui.set_enabled(!self.is_converting);
                                ui.vertical_centered(|ui| {
                                    if ui.button("Clear").clicked() {
                                        to_remove = Some(i);
                                    }
                                });
                            });
                            ui.allocate_ui(egui::vec2(status_w, 25.0), |ui| {
                                ui.vertical_centered(|ui| match file.status {
                                    FileStatus::Idle => {
                                        ui.label("Idle");
                                    }
                                    FileStatus::Processing => {
                                        ui.label(egui::RichText::new("-").strong());
                                    }
                                    FileStatus::Done => {
                                        ui.label(
                                            egui::RichText::new("✔").color(egui::Color32::GREEN),
                                        );
                                    }
                                    FileStatus::Failed => {
                                        ui.label(
                                            egui::RichText::new("✘").color(egui::Color32::RED),
                                        );
                                    }
                                });
                            });
                            ui.end_row();
                        }
                        if let Some(index) = to_remove {
                            self.files.remove(index);
                        }
                    });
            });
        });
    }
}

/*
 * collect_files_recursive - Function
 * Expects: NA
 * Does: gets all the paths of media files recursively inside the given folder and adds it to the media section
 */
fn collect_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files_recursive(&path, files);
            } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                let media_exts = [
                    "mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv", "flv", "mp3", "wav", "flac",
                    "ogg", "m4a", "aac", "opus", "srt", "ass", "ssa", "vtt",
                ];
                if media_exts.contains(&ext.to_lowercase().as_str()) {
                    files.push(path);
                }
            }
        }
    }
}

/*
 * get_media_info - Function
 * Expects: The file to be a media file
 * Does: Returns the media information for the given file
 * Returns: Media information for the given file
 */
fn get_media_info(path: &str) -> (String, String, String) {
    let probe = |stream_type: &str| -> String {
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                stream_type,
                "-show_entries",
                "stream=codec_name",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                path,
            ])
            .output();
        match output {
            Ok(out) => {
                let result = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if result.is_empty() {
                    "None".to_string()
                } else {
                    result.lines().next().unwrap_or("None").to_string()
                }
            }
            Err(_) => "Invalid".to_string(),
        }
    };
    (probe("v:0"), probe("a:0"), probe("s:0"))
}
