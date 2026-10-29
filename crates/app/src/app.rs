use std::path::PathBuf;
use std::sync::{Arc, Mutex, atomic::Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use eframe::{egui, egui_wgpu};

use crate::assets::{AssetLoader, AssetSelection, DecodedImage, PreviewAssets};
use crate::gpu::{GpuImage, PreviewRenderer};

pub type SmokeOutcome = Arc<Mutex<Option<Result<(), String>>>>;

#[derive(Default)]
pub struct LaunchOptions {
    pub p2_smoke: bool,
    pub playtest_exe: Option<PathBuf>,
    pub game_dir: Option<PathBuf>,
    pub map: Option<String>,
    pub new_map: Option<crate::new_map::NewMapOptions>,
    pub tmp: Option<String>,
    pub shp: Option<String>,
    pub sub_tile: Option<usize>,
    pub frame: Option<usize>,
    pub font: Option<PathBuf>,
    pub smoke_output: Option<PathBuf>,
}

struct Loaded {
    assets: PreviewAssets,
    tile: Arc<GpuImage>,
    sprite: Arc<GpuImage>,
}

struct Camera {
    zoom: f32,
    pan: egui::Vec2,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            zoom: 3.0,
            pan: egui::Vec2::ZERO,
        }
    }
}

pub struct EditorApp {
    map: crate::map_view::MapView,
    show_map: bool,
    selection: AssetSelection,
    directory_text: String,
    loader: AssetLoader,
    generation: u64,
    loading: bool,
    loaded: Option<Loaded>,
    error: Option<String>,
    renderer: Arc<PreviewRenderer>,
    gpu: egui_wgpu::RenderState,
    cameras: [Camera; 2],
    font_loaded: bool,
    smoke_output: Option<PathBuf>,
    smoke_outcome: SmokeOutcome,
    screenshot_requested: bool,
    smoke_verified: bool,
    resource_compact_started: Option<Instant>,
    started: Instant,
    p2_smoke: bool,
}

impl EditorApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        options: LaunchOptions,
        outcome: SmokeOutcome,
    ) -> Result<Self> {
        let font_loaded = install_font(&cc.egui_ctx, options.font)?;
        crate::ui_theme::install(&cc.egui_ctx);
        let gpu = cc.wgpu_render_state.clone().context("P0 需要 wgpu 后端")?;
        log::info!(
            "GPU: {:?}, target: {:?}",
            gpu.adapter.get_info(),
            gpu.target_format
        );
        let renderer = Arc::new(PreviewRenderer::new(&gpu.device, gpu.target_format));
        let mut selection: AssetSelection = if options.smoke_output.is_none() {
            cc.storage
                .and_then(|s| eframe::get_value(s, "asset-selection"))
                .unwrap_or_default()
        } else {
            AssetSelection::default()
        };
        if let Some(directory) = options.game_dir {
            selection.directory = directory;
        } else if selection.directory.as_os_str().is_empty()
            && let Some(directory) = std::env::var_os("RA2_DIR")
        {
            selection.directory = directory.into();
        }
        if let Some(name) = options.tmp {
            selection.tmp_name = name;
        }
        if let Some(name) = options.shp {
            selection.shp_name = name;
        }
        if let Some(index) = options.sub_tile {
            selection.sub_tile = index;
        }
        if let Some(index) = options.frame {
            selection.frame = index;
        }
        let initial_map = options.map;
        let show_map =
            initial_map.is_some() || options.new_map.is_some() || options.smoke_output.is_none();
        let mut app = Self {
            map: crate::map_view::MapView::new(
                cc.egui_ctx.clone(),
                gpu.clone(),
                options
                    .smoke_output
                    .as_ref()
                    .map(|p| p.join("editor-data"))
                    .unwrap_or_else(crate::editor_io::data_root),
            )?,
            show_map,
            directory_text: selection.directory.to_string_lossy().into_owned(),
            selection,
            loader: AssetLoader::new(cc.egui_ctx.clone())?,
            generation: 0,
            loading: false,
            loaded: None,
            error: None,
            renderer,
            gpu,
            cameras: Default::default(),
            font_loaded,
            smoke_output: options.smoke_output,
            smoke_outcome: outcome,
            screenshot_requested: false,
            smoke_verified: false,
            resource_compact_started: None,
            started: Instant::now(),
            p2_smoke: options.p2_smoke,
        };
        if let Some(exe) = options.playtest_exe {
            app.map.editor.executable = exe.display().to_string();
            app.map.p2_playtest = options.p2_smoke;
        }
        if let Some(options) = options.new_map {
            app.map.create(app.selection.directory.clone(), options);
        } else if let Some(name) = initial_map {
            app.map.open(app.selection.directory.clone(), name);
        } else if !app.show_map && !app.selection.directory.as_os_str().is_empty() {
            app.load(true);
        }
        Ok(app)
    }

    fn load(&mut self, refresh: bool) {
        self.generation += 1;
        self.loading = true;
        self.error = None;
        // Never show old assets under a newly requested directory/name.
        self.loaded = None;
        self.selection.directory = PathBuf::from(self.directory_text.trim());
        if let Err(error) = self
            .loader
            .request(self.generation, self.selection.clone(), refresh)
        {
            self.error = Some(format!("{error:#}"));
            self.loading = false;
        }
    }

    fn poll_loader(&mut self) {
        while let Ok(message) = self.loader.results.try_recv() {
            if message.generation != self.generation {
                continue;
            }
            self.loading = false;
            let result = message.result.and_then(|assets| {
                let tile = self
                    .renderer
                    .upload(&self.gpu.device, &self.gpu.queue, &assets.tile)?;
                let sprite =
                    self.renderer
                        .upload(&self.gpu.device, &self.gpu.queue, &assets.sprite)?;
                Ok(Loaded {
                    assets,
                    tile,
                    sprite,
                })
            });
            match result {
                Ok(loaded) => {
                    self.cameras = Default::default();
                    self.loaded = Some(loaded);
                }
                Err(error) => {
                    log::error!("Asset load failed: {error:#}");
                    self.error = Some(format!("{error:#}"));
                }
            }
        }
    }

    fn finish_smoke(&mut self, ctx: &egui::Context, result: Result<()>) {
        let outcome = result.map_err(|e| format!("{e:#}"));
        if let Err(error) = &outcome {
            log::error!("Editor smoke failed: {error}");
            if let Some(output) = &self.smoke_output {
                let report = serde_json::json!({"passed":false,"error":error});
                let _ = std::fs::write(output.join("report.json"), report.to_string());
            }
        }
        *self.smoke_outcome.lock().expect("smoke outcome") = Some(outcome);
        self.map.close_smoke(ctx);
    }

    fn smoke(&mut self, ctx: &egui::Context) {
        let Some(output) = self.smoke_output.clone() else {
            return;
        };
        if self.smoke_outcome.lock().expect("smoke outcome").is_some() {
            return;
        }
        if self.started.elapsed() > Duration::from_secs(60) {
            self.finish_smoke(ctx, Err(anyhow::anyhow!("烟测等待加载/截图超过 60 秒")));
            return;
        }
        if let Some(error) = &self.error {
            self.finish_smoke(ctx, Err(anyhow::anyhow!(error.clone())));
            return;
        }
        if let Some(loaded) = &self.loaded {
            if !self.smoke_verified {
                let result = (|| -> Result<()> {
                    std::fs::create_dir_all(&output)?;
                    let tile_check = self.renderer.verify_and_save(
                        &self.gpu.device,
                        &self.gpu.queue,
                        &loaded.tile,
                        &loaded.assets.tile,
                        &output.join("tmp.png"),
                    )?;
                    let sprite_check = self.renderer.verify_and_save(
                        &self.gpu.device,
                        &self.gpu.queue,
                        &loaded.sprite,
                        &loaded.assets.sprite,
                        &output.join("shp.png"),
                    )?;
                    let info = self.gpu.adapter.get_info();
                    let report = serde_json::json!({
                        "game_directory": self.selection.directory,
                        "theater": "TEMPERATE", "wgpu_version": "29.0.4",
                        "gpu": { "name": info.name, "backend": format!("{:?}", info.backend), "target": format!("{:?}", self.gpu.target_format) },
                        "device_owner": "eframe", "additional_device_created": false,
                        "tile": { "asset": loaded.assets.tile.info, "gpu_readback": tile_check },
                        "sprite": { "asset": loaded.assets.sprite.info, "gpu_readback": sprite_check },
                        "archives": loaded.assets.archive_count, "tileset_slots": loaded.assets.tileset_slots,
                    });
                    std::fs::write(
                        output.join("report.json"),
                        serde_json::to_vec_pretty(&report)?,
                    )?;
                    Ok(())
                })();
                if let Err(error) = result {
                    self.finish_smoke(ctx, Err(error));
                    return;
                }
                self.smoke_verified = true;
            }
            // Require both custom callbacks to have actually drawn in the host.
            if loaded.tile.paints.load(Ordering::Relaxed) >= 2
                && loaded.sprite.paints.load(Ordering::Relaxed) >= 2
                && !self.screenshot_requested
                && self
                    .resource_compact_started
                    .is_none_or(|t| t.elapsed() >= Duration::from_millis(350))
            {
                self.screenshot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
        }
        let screenshot = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = screenshot {
            let result = (|| -> Result<()> {
                ensure!(
                    self.smoke_verified && self.screenshot_requested,
                    "截图早于 GPU 验证"
                );
                let rgba: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                image::save_buffer(
                    output.join(if self.resource_compact_started.is_some() {
                        "window-compact.png"
                    } else {
                        "window.png"
                    }),
                    &rgba,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                )?;
                let report_path = output.join("report.json");
                let mut report: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&report_path)?)?;
                let loaded = self.loaded.as_ref().context("烟测丢失预览资源")?;
                report["ui_callback_draws"] = serde_json::json!({
                    "tile": loaded.tile.paints.load(Ordering::Relaxed),
                    "sprite": loaded.sprite.paints.load(Ordering::Relaxed),
                });
                let compact = self.resource_compact_started.is_some();
                report[if compact {
                    "compact_window_capture"
                } else {
                    "window_capture"
                }] = serde_json::json!({ "width": image.size[0], "height": image.size[1] });
                report["passed"] = compact.into();
                std::fs::write(report_path, serde_json::to_vec_pretty(&report)?)?;
                Ok(())
            })();
            if result.is_ok() && self.resource_compact_started.is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(900., 600.)));
                self.resource_compact_started = Some(Instant::now());
                self.screenshot_requested = false;
            } else {
                if result.is_ok() {
                    println!(
                        "P0 resource + compact UI smoke passed: {}",
                        output.display()
                    );
                }
                self.finish_smoke(ctx, result);
            }
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("资源目录");
        let path_response = ui.add(
            egui::TextEdit::singleline(&mut self.directory_text)
                .desired_width(f32::INFINITY)
                .hint_text("游戏安装目录…"),
        );
        let enter = path_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            if ui.button("选择目录…").clicked()
                && let Some(directory) = rfd::FileDialog::new()
                    .set_title("选择尤里的复仇游戏目录")
                    .pick_folder()
            {
                self.directory_text = directory.to_string_lossy().into_owned();
                self.load(true);
            }
            if ui.button("加载 / 重载").clicked() || enter {
                self.load(true);
            }
        });
        ui.label("Theater：TEMPERATE");
        ui.separator();
        ui.heading("地形 TMP");
        ui.add(
            egui::TextEdit::singleline(&mut self.selection.tmp_name)
                .desired_width(f32::INFINITY)
                .hint_text("例如 clear01.tem"),
        );
        ui.horizontal(|ui| {
            ui.label("子块");
            ui.add(egui::DragValue::new(&mut self.selection.sub_tile).range(0..=65535));
        });
        ui.heading("精灵 SHP");
        ui.add(
            egui::TextEdit::singleline(&mut self.selection.shp_name)
                .desired_width(f32::INFINITY)
                .hint_text("例如 gi.shp"),
        );
        ui.horizontal(|ui| {
            ui.label("帧");
            ui.add(egui::DragValue::new(&mut self.selection.frame).range(0..=65535));
        });
        if ui.add(crate::ui_theme::primary("预览所选资源")).clicked() {
            self.load(false);
        }
        ui.small("SHP 使用 unittem.pal；当前适合预览步兵和建筑精灵。显示选中帧的裁剪区域。");
        ui.separator();
        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("正在读取资源…");
            });
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if let Some(loaded) = &self.loaded {
            ui.label(format!("已挂载 {} 个归档", loaded.assets.archive_count));
            ui.label(format!("{} 个地形索引槽位", loaded.assets.tileset_slots));
        }
        if !self.font_loaded {
            ui.colored_label(
                egui::Color32::YELLOW,
                "未找到中文字体，可通过 --font 指定字体文件。",
            );
        }
        ui.separator();
        ui.label("拖动平移 · 滚轮缩放 · 双击适应视图");
        ui.collapsing("图形设备", |ui| {
            let info = self.gpu.adapter.get_info();
            ui.label(info.name);
            ui.label(format!("{:?} / {:?}", info.backend, self.gpu.target_format));
            ui.small("eframe 与两个预览共用 GPU Device；wgpu 29.0.4");
        });
    }
}

impl eframe::App for EditorApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.p2_smoke {
            self.map.smoke_input(raw_input);
        }
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_loader();
        self.map.poll(ctx);
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
        if let Some(path) = dropped {
            self.show_map = true;
            self.map.open(
                PathBuf::from(self.directory_text.trim()),
                path.to_string_lossy().into_owned(),
            );
        }
        if self.smoke_outcome.lock().expect("smoke outcome").is_some() {
            return;
        }
        if self.show_map {
            if let Some(output) = self.smoke_output.clone()
                && let Some(result) = if self.p2_smoke {
                    self.map.smoke_p2(ctx, &self.gpu, &output)
                } else {
                    self.map.smoke(ctx, &self.gpu, &output)
                }
            {
                self.finish_smoke(ctx, result);
            }
        } else {
            self.smoke(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        use crate::ui_theme as theme;
        self.map
            .settings_windows(ui.ctx(), &mut self.directory_text);
        self.map.dialogs(ui.ctx());
        egui::Panel::top("workbench-brand")
            .frame(
                egui::Frame::new()
                    .fill(theme::BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (logo, _) =
                        ui.allocate_exact_size(egui::vec2(26., 26.), egui::Sense::hover());
                    ui.painter().rect_filled(logo, 6, theme::ACCENT);
                    theme::icon(ui.painter(), logo, theme::Icon::Terrain, theme::BACKGROUND);
                    ui.add_space(2.);
                    ui.label(egui::RichText::new("YR MAP EDITOR").size(16.).strong());
                    ui.add_space(10.);
                    if ui
                        .add_sized(
                            [82., 28.],
                            egui::Button::new("地图编辑").selected(self.show_map),
                        )
                        .clicked()
                    {
                        self.show_map = true;
                    }
                    if ui
                        .add_sized(
                            [82., 28.],
                            egui::Button::new("资源预览").selected(!self.show_map),
                        )
                        .clicked()
                    {
                        self.show_map = false;
                        if self.loaded.is_none() && !self.loading {
                            self.load(false);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("尤里的复仇  /  VERA20k")
                                .size(11.)
                                .color(theme::MUTED),
                        );
                    });
                });
            });
        if self.show_map {
            egui::Panel::top("document-actions")
                .frame(
                    egui::Frame::new()
                        .fill(theme::PANEL)
                        .inner_margin(egui::Margin::symmetric(12, 6))
                        .stroke(egui::Stroke::new(1., theme::BORDER)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| self.map.toolbar(ui, &self.directory_text));
                });
            egui::Panel::bottom("workbench-status")
                .frame(
                    egui::Frame::new()
                        .fill(theme::BACKGROUND)
                        .inner_margin(egui::Margin::symmetric(12, 5)),
                )
                .show(ui, |ui| self.map.status_bar(ui));
            egui::Panel::left("tool-library")
                .default_size(252.)
                .min_size(218.)
                .max_size(380.)
                .resizable(true)
                .frame(theme::panel_frame())
                .show(ui, |ui| self.map.toolbox(ui));
            egui::Panel::right("property-inspector")
                .default_size(248.)
                .min_size(218.)
                .max_size(360.)
                .resizable(true)
                .frame(theme::panel_frame())
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("inspector-scroll")
                        .show(ui, |ui| self.map.inspector(ui));
                });
        } else {
            egui::Panel::left("resources")
                .default_size(270.)
                .min_size(230.)
                .resizable(true)
                .frame(theme::panel_frame())
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(crate::ui_theme::BACKGROUND)
                    .inner_margin(if self.show_map { 0 } else { 16 }),
            )
            .show(ui, |ui| {
                if self.show_map {
                    self.map.canvas(ui, &self.gpu);
                    return;
                }
                if let Some(loaded) = &self.loaded {
                    ui.columns(2, |columns| {
                        image_panel(
                            &mut columns[0],
                            "地形",
                            &loaded.assets.tile,
                            &loaded.tile,
                            &self.renderer,
                            &self.gpu.queue,
                            &mut self.cameras[0],
                        );
                        image_panel(
                            &mut columns[1],
                            "精灵",
                            &loaded.assets.sprite,
                            &loaded.sprite,
                            &self.renderer,
                            &self.gpu.queue,
                            &mut self.cameras[1],
                        );
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(if self.loading {
                            "正在加载真实游戏资源…"
                        } else {
                            "选择游戏目录，预览 TMP 地形与 SHP 精灵。"
                        });
                    });
                }
            });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.smoke_output.is_none() {
            self.selection.directory = PathBuf::from(self.directory_text.trim());
            eframe::set_value(storage, "asset-selection", &self.selection);
        }
    }
}

fn image_panel(
    ui: &mut egui::Ui,
    title: &str,
    image: &DecodedImage,
    gpu_image: &Arc<GpuImage>,
    renderer: &Arc<PreviewRenderer>,
    queue: &wgpu::Queue,
    camera: &mut Camera,
) {
    crate::ui_theme::section(ui, title, "资源预览");
    ui.add(egui::Label::new(egui::RichText::new(&image.info.name).strong().size(16.)).truncate())
        .on_hover_text(&image.info.name);
    ui.label(
        egui::RichText::new(format!(
            "{} × {}  ·  索引 {} / {}  ·  偏移 {}, {}",
            image.info.width,
            image.info.height,
            image.info.index,
            image.info.count.saturating_sub(1),
            image.info.offset[0],
            image.info.offset[1]
        ))
        .size(11.)
        .color(crate::ui_theme::MUTED),
    );
    egui::CollapsingHeader::new("来源与调色板")
        .id_salt(("asset-origin", title))
        .show(ui, |ui| {
            ui.add(egui::Label::new(format!("来源：{}", image.info.source)).truncate())
                .on_hover_text(&image.info.source);
            ui.add(egui::Label::new(format!("调色板：{}", image.info.palette)).truncate())
                .on_hover_text(&image.info.palette_source);
        });
    let reset = ui
        .horizontal(|ui| {
            let reset = ui.button("适应视图").clicked();
            if ui.button("1:1").clicked() {
                camera.zoom = 1.0 / ui.ctx().pixels_per_point();
                camera.pan = egui::Vec2::ZERO;
            }
            ui.label(format!(
                "{:.0}%",
                camera.zoom * ui.ctx().pixels_per_point() * 100.0
            ));
            reset
        })
        .inner;
    let size = ui.available_size().max(egui::vec2(1.0, 1.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::drag());
    ui.painter()
        .rect_filled(rect, 5, crate::ui_theme::BACKGROUND);
    ui.painter().rect_stroke(
        rect,
        5,
        egui::Stroke::new(1., crate::ui_theme::BORDER),
        egui::StrokeKind::Inside,
    );
    let image_size = egui::vec2(
        image.info.width.max(1) as f32,
        image.info.height.max(1) as f32,
    );
    if reset || response.double_clicked() {
        camera.zoom = ((size.x - 32.0).max(1.0) / image_size.x)
            .min((size.y - 32.0).max(1.0) / image_size.y)
            .clamp(0.1, 16.0);
        camera.pan = egui::Vec2::ZERO;
    }
    camera.pan += response.drag_delta();
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        let next_zoom = (camera.zoom * (scroll * 0.003).exp()).clamp(0.1, 16.0);
        if let Some(pointer) = response.hover_pos() {
            let from_center = pointer - rect.center();
            camera.pan = from_center - (from_center - camera.pan) * (next_zoom / camera.zoom);
        }
        camera.zoom = next_zoom;
    }
    // The host callback viewport uses physical pixels; this ratio is in egui
    // points, so DevicePixelRatio affects the image and the panel together.
    ui.painter().add(renderer.callback(
        queue,
        gpu_image,
        rect,
        image_size * camera.zoom,
        camera.pan,
    ));
}

fn install_font(ctx: &egui::Context, explicit: Option<PathBuf>) -> Result<bool> {
    let candidates = if let Some(path) = explicit {
        ensure!(path.is_file(), "指定的字体不存在：{}", path.display());
        vec![path]
    } else {
        let mut paths = Vec::new();
        if let Some(windir) = std::env::var_os("WINDIR") {
            paths.push(PathBuf::from(windir).join("Fonts/msyh.ttc"));
        }
        paths.extend([
            PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        ]);
        paths
    };
    for path in candidates {
        if path.is_file() {
            let bytes = std::fs::read(&path)
                .with_context(|| format!("读取字体失败：{}", path.display()))?;
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("cjk".into(), Arc::new(egui::FontData::from_owned(bytes)));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".into());
            }
            ctx.set_fonts(fonts);
            return Ok(true);
        }
    }
    Ok(false)
}
