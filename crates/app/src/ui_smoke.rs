//! Capture the real native UI at desktop and minimum window sizes.
use super::*;
use crate::editing::Tool;
use ra2_map_document::mission::{self, Kind};
use ra2_map_scene::terrain_tools::TerrainMode;

#[derive(Default)]
pub(super) struct UiSmoke {
    index: usize,
    phase: u8,
    started: Option<Instant>,
    captures: Vec<serde_json::Value>,
    before: Option<Vec<u8>>,
}

#[derive(Clone, Copy)]
pub(super) enum Panel {
    Library(u8),
    Terrain(TerrainMode),
    Tool(Tool),
    Object,
    Region,
    Settings,
    New,
    Random,
    Unsaved,
    Help,
    Playtest,
    Mission(Kind),
    Campaign,
    BaseNodes,
    MapProperties(u8),
}

pub(super) fn panels() -> Vec<(String, Panel)> {
    let mut panels = vec![];
    for (tab, name) in ["terrain", "objects", "waypoints", "overlays", "animations"]
        .into_iter()
        .enumerate()
    {
        panels.push((format!("library-{name}"), Panel::Library(tab as u8)));
    }
    for mode in TerrainMode::ALL {
        panels.push((
            format!("terrain-{mode:?}").to_lowercase(),
            Panel::Terrain(mode),
        ));
    }
    for tool in [
        Tool::Ore,
        Tool::Gems,
        Tool::Overlay,
        Tool::Erase,
        Tool::Place,
        Tool::Move,
        Tool::Clone,
        Tool::Waypoint,
        Tool::Pick,
        Tool::Paste,
        Tool::RegionMove,
    ] {
        panels.push((format!("tool-{tool:?}").to_lowercase(), Panel::Tool(tool)));
    }
    panels.extend([
        ("object-properties".into(), Panel::Object),
        ("region-options".into(), Panel::Region),
        ("project-settings".into(), Panel::Settings),
        ("new-map".into(), Panel::New),
        ("random-map".into(), Panel::Random),
        ("unsaved-map".into(), Panel::Unsaved),
        ("help".into(), Panel::Help),
        ("playtest".into(), Panel::Playtest),
        ("campaign-package".into(), Panel::Campaign),
        ("map-properties".into(), Panel::MapProperties(0)),
        ("map-lighting".into(), Panel::MapProperties(1)),
        ("map-crates".into(), Panel::MapProperties(2)),
        ("map-info".into(), Panel::MapProperties(3)),
        ("base-nodes".into(), Panel::BaseNodes),
    ]);
    for kind in Kind::ALL {
        panels.push((
            format!("mission-{kind:?}").to_lowercase(),
            Panel::Mission(kind),
        ));
    }
    panels
}

impl MapView {
    pub(super) fn ui_smoke_step(&mut self, ctx: &egui::Context, output: &Path) -> Result<bool> {
        let panels = panels();
        if self.ui_smoke.index == panels.len() * 2 {
            ensure!(
                self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                    == *self.ui_smoke.before.as_ref().unwrap(),
                "UI capture changed the map"
            );
            self.pending = None;
            let path = output.join("report.json");
            let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            report["ui_layout"] =
                serde_json::json!({"captures":self.ui_smoke.captures, "no_document_writes":true});
            report["passed"] = true.into();
            std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
            let mut gallery = String::from(
                "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>编辑器界面复查</title><style>body{background:#11151a;color:#dce3ea;font:15px system-ui;margin:24px}h1{font-size:24px}p{color:#8b9aa8}main{display:grid;grid-template-columns:repeat(auto-fit,minmax(360px,1fr));gap:20px}figure{margin:0;padding:12px;background:#191e24;border:1px solid #313a44;border-radius:8px}img{width:100%;height:auto;display:block}figcaption{margin-top:10px;color:#e5b45d}</style><h1>编辑器界面复查</h1><p>1280×800 与 900×600 逻辑尺寸；点击截图查看原图。</p><main>",
            );
            for capture in &self.ui_smoke.captures {
                let file = capture["image"].as_str().unwrap();
                let panel = capture["panel"].as_str().unwrap();
                gallery.push_str(&format!("<figure><a href=\"{file}\"><img loading=\"lazy\" src=\"{file}\" alt=\"{panel}\"></a><figcaption>{} px · {panel}</figcaption></figure>", capture["logical_size"][0]));
            }
            gallery.push_str("</main></html>");
            std::fs::write(output.join("index.html"), gallery)?;
            println!(
                "P2/P3/P4 + interaction + UI layout smoke passed: {}",
                output.display()
            );
            return Ok(true);
        }
        let compact = self.ui_smoke.index >= panels.len();
        let size = if compact {
            egui::vec2(900., 600.)
        } else {
            egui::vec2(1280., 800.)
        };
        let (name, panel) = &panels[self.ui_smoke.index % panels.len()];
        let file = format!("ui-{}-{name}.png", if compact { "900" } else { "1280" });
        match self.ui_smoke.phase {
            0 => {
                if self.ui_smoke.before.is_none() {
                    self.ui_smoke.before =
                        Some(self.editor.doc.as_ref().unwrap().snapshot().bytes()?);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                egui::Popup::close_all(ctx);
                self.workbench.settings = false;
                self.workbench.new_map = false;
                self.workbench.random_map = false;
                self.workbench.help = false;
                self.workbench.playtest = false;
                self.workbench.mission.open = false;
                self.workbench.mission.base.stop();
                self.workbench.mission.campaign.open = false;
                self.workbench.map_settings.open = false;
                self.pending = None;
                self.editor.select(None);
                self.editor.tool = Tool::Select;
                match *panel {
                    Panel::Library(tab) => self.editor.show_library(tab, "", ""),
                    Panel::Terrain(mode) => {
                        self.editor.show_library(0, "", "");
                        self.editor.activate_terrain_mode(mode);
                    }
                    Panel::Object => {
                        let id = self
                            .editor
                            .records
                            .iter()
                            .find(|(_, r)| r.section == "Structures")
                            .map(|(&id, _)| id);
                        self.editor.select(id);
                    }
                    Panel::Tool(tool) => {
                        if matches!(tool, Tool::Move | Tool::Clone) {
                            let id = self
                                .editor
                                .records
                                .iter()
                                .find(|(_, r)| r.section == "Structures")
                                .map(|(&id, _)| id);
                            self.editor.select(id);
                        }
                        self.editor.tool = tool;
                        self.editor.show_library(
                            match tool {
                                Tool::Place => 1,
                                Tool::Waypoint => 2,
                                Tool::Ore | Tool::Gems | Tool::Overlay | Tool::Erase => 3,
                                _ => 0,
                            },
                            "",
                            "",
                        );
                    }
                    Panel::Region => self.editor.tool = Tool::Region,
                    Panel::Settings => self.workbench.settings = true,
                    Panel::MapProperties(tab) => {
                        self.workbench.map_settings.open = true;
                        self.workbench.map_settings.tab = tab;
                    }
                    Panel::New => self.workbench.new_map = true,
                    Panel::Random => self.workbench.random_map = true,
                    Panel::Unsaved => self.pending = Some(Leave::Close),
                    Panel::Help => self.workbench.help = true,
                    Panel::Playtest => self.workbench.playtest = true,
                    Panel::Campaign => self
                        .workbench
                        .mission
                        .campaign
                        .smoke_open(&output.join("ai-authoring.map")),
                    Panel::BaseNodes => {
                        let house = mission::ids(self.editor.doc.as_ref().unwrap(), Kind::House)
                            .first()
                            .cloned()
                            .unwrap_or_default();
                        self.workbench
                            .mission
                            .navigate(&self.editor, Kind::House, house.clone());
                        self.workbench.mission.base.start(house);
                    }
                    Panel::Mission(kind) => {
                        let id = mission::ids(self.editor.doc.as_ref().unwrap(), kind)
                            .first()
                            .cloned()
                            .unwrap_or_default();
                        self.workbench.mission.navigate(&self.editor, kind, id);
                    }
                }
                self.ui_smoke.started = Some(Instant::now());
                self.ui_smoke.phase = 1;
            }
            1 => {
                if self.ui_smoke.started.unwrap().elapsed() < Duration::from_millis(350) {
                    return Ok(false);
                }
                let bounds = ctx.content_rect();
                ensure!(
                    (bounds.size() - size).length() < 4.,
                    "native window did not resize: {:?} vs {:?}",
                    bounds.size(),
                    size
                );
                let window = match *panel {
                    Panel::Settings => Some("project-settings"),
                    Panel::New => Some("new-map"),
                    Panel::Random => Some("random-map"),
                    Panel::Unsaved => Some("unsaved-map"),
                    Panel::Help => Some("interaction-help"),
                    Panel::Playtest => Some("playtest-settings"),
                    Panel::Mission(_) => Some("mission-authoring"),
                    Panel::Campaign => Some("campaign-project"),
                    Panel::BaseNodes => Some("base-nodes-canvas"),
                    Panel::MapProperties(_) => Some("map-properties"),
                    _ => None,
                };
                let rect = if let Some(id) = window {
                    let rect = ctx
                        .memory(|m| m.area_rect(egui::Id::unique(id)))
                        .context("missing dialog bounds")?;
                    ensure!(
                        bounds.expand(2.).contains_rect(rect),
                        "{name} exceeds {:?} viewport: {:?}",
                        size,
                        rect
                    );
                    Some([rect.left(), rect.top(), rect.width(), rect.height()])
                } else {
                    let rect = self.workbench.canvas_rect.context("no canvas rect")?;
                    ensure!(
                        bounds.contains_rect(rect) && rect.width() > 200. && rect.height() > 200.,
                        "canvas was squeezed or clipped: {:?}",
                        rect
                    );
                    None
                };
                self.ui_smoke.captures.push(serde_json::json!({"panel":name,"logical_size":[size.x,size.y],"pixels_per_point":ctx.pixels_per_point(),"window_rect":rect,"image":file}));
                crate::screen_capture::request(ctx);
                self.ui_smoke.phase = 2;
            }
            _ => {
                let capture = crate::screen_capture::take(ctx);
                let Some(image) = capture else {
                    return Ok(false);
                };
                let rgba: Vec<_> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                image::save_buffer(
                    output.join(file),
                    &rgba,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                )?;
                self.ui_smoke.index += 1;
                self.ui_smoke.phase = 0;
            }
        }
        Ok(false)
    }
}
