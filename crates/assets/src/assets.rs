//! Read-only P0 adapter. Format parsing and palette semantics belong to core.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use anyhow::{Context, Result, bail, ensure};

use serde::{Deserialize, Serialize};
use vera20k_core::assets::{
    asset_manager::{AssetManager, MediaArchiveMode},
    shp_file::ShpFile,
    tmp_file::TmpFile,
};
use vera20k_core::map::theater::{self, TheaterData};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetSelection {
    pub directory: PathBuf,
    pub tmp_name: String,
    pub sub_tile: usize,
    pub shp_name: String,
    pub frame: usize,
}

impl Default for AssetSelection {
    fn default() -> Self {
        Self {
            directory: PathBuf::new(),
            tmp_name: "clear01.tem".into(),
            sub_tile: 0,
            shp_name: "gi.shp".into(),
            frame: 0,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct AssetInfo {
    pub name: String,
    pub source: String,
    pub palette: String,
    pub palette_source: String,
    pub index: usize,
    pub count: usize,
    pub offset: [i32; 2],
    pub width: u32,
    pub height: u32,
}

#[derive(Clone)]
pub struct DecodedImage {
    /// Original palette indices; retained for exact 16..31 house remap.
    pub indices: Vec<u8>,
    pub info: AssetInfo,
    pub rgba: Vec<u8>,
}

pub struct PreviewAssets {
    pub tile: DecodedImage,
    pub sprite: DecodedImage,
    pub archive_count: usize,
    pub tileset_slots: usize,
}

pub struct Install {
    pub revision: std::sync::Arc<crate::profile::Revision>,
    directory: PathBuf,
    pub manager: AssetManager,
    pub theater: TheaterData,
    pub theater_name: String,
    pub catalog: std::sync::Arc<crate::catalog::Catalog>,
    palette: std::sync::OnceLock<std::sync::Arc<crate::catalog::TerrainPalette>>,
    sprites: std::sync::OnceLock<std::sync::Arc<crate::sprites::SpritePalette>>,
    mission_resources:
        std::sync::OnceLock<std::sync::Arc<crate::mission_resources::MissionResources>>,
    fixed_teams: std::sync::OnceLock<
        Result<std::sync::Arc<vera20k_core::rules::team_ai_ini::TeamAiIniRegistry>, String>,
    >,
}

impl Install {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn from_probe_root(directory: &Path) -> Result<Self> {
        let mut manager =
            AssetManager::from_ini_probe_root(directory).context("无法打开合成资源目录")?;
        let theater =
            theater::load_theater(&mut manager, "TEMPERATE").context("合成目录缺少温带资源")?;
        Ok(Self {
            revision: std::sync::Arc::new(crate::profile::Revision::scan(directory)?),
            directory: directory.to_owned(),
            manager,
            theater,
            theater_name: "TEMPERATE".into(),
            catalog: Default::default(),
            palette: Default::default(),
            sprites: Default::default(),
            mission_resources: Default::default(),
            fixed_teams: Default::default(),
        })
    }

    pub fn open(directory: &Path) -> Result<Self> {
        Self::open_controlled(directory, &|| false)
    }
    pub fn open_controlled(directory: &Path, cancelled: &dyn Fn() -> bool) -> Result<Self> {
        ensure!(!cancelled(), "resource load cancelled");
        ensure!(
            directory.is_dir(),
            "游戏目录不存在：{}",
            directory.display()
        );
        let mut manager = AssetManager::new(directory, MediaArchiveMode::STOCK_DIGITAL)
            .context("无法打开 YR 资源归档")?;
        let theater = theater::load_theater(&mut manager, "TEMPERATE").context(
            "无法加载 TEMPERATE；请检查 temperatmd.ini、isotem.pal、unittem.pal 和对应 MIX",
        )?;
        ensure!(!cancelled(), "resource load cancelled");
        let catalog = crate::catalog::Catalog::load(&manager, &theater);
        Ok(Self {
            revision: std::sync::Arc::new(crate::profile::Revision::scan_controlled(
                directory, cancelled,
            )?),
            directory: directory.to_owned(),
            manager,
            theater,
            theater_name: "TEMPERATE".into(),
            catalog,
            palette: Default::default(),
            sprites: Default::default(),
            mission_resources: Default::default(),
            fixed_teams: Default::default(),
        })
    }
    pub fn mission_resources(&self) -> std::sync::Arc<crate::mission_resources::MissionResources> {
        self.mission_resources
            .get_or_init(|| std::sync::Arc::new(crate::mission_resources::load(&self.manager)))
            .clone()
    }
    pub fn fixed_team_definitions(
        &self,
    ) -> Result<std::sync::Arc<vera20k_core::rules::team_ai_ini::TeamAiIniRegistry>, String> {
        self.fixed_teams
            .get_or_init(|| {
                vera20k_core::rules::team_ai_ini::TeamAiIniRegistry::load_retail(
                    &self.manager,
                    &vera20k_core::rules::ini_parser::IniFile::empty(),
                    false,
                )
                .map(std::sync::Arc::new)
            })
            .clone()
    }
    pub fn terrain_palette(&self) -> std::sync::Arc<crate::catalog::TerrainPalette> {
        self.palette
            .get_or_init(|| {
                std::sync::Arc::new(self.catalog.terrain_palette(&self.manager, &self.theater))
            })
            .clone()
    }
    pub fn activate_theater(&mut self, name: &str) -> Result<()> {
        if self.theater_name.eq_ignore_ascii_case(name) {
            return Ok(());
        }
        let mut manager = AssetManager::new(&self.directory, MediaArchiveMode::STOCK_DIGITAL)?;
        let theater = theater::load_theater(&mut manager, name)
            .with_context(|| format!("无法加载 {name} 环境资源"))?;
        self.catalog = crate::catalog::Catalog::load(&manager, &theater);
        self.manager = manager;
        self.fixed_teams = Default::default();
        self.theater = theater;
        self.theater_name = name.to_ascii_uppercase();
        self.palette.take();
        self.sprites.take();
        Ok(())
    }
    pub fn sprite_palette(
        &self,
        catalog: &std::sync::Arc<crate::catalog::Catalog>,
    ) -> std::sync::Arc<crate::sprites::SpritePalette> {
        if std::sync::Arc::ptr_eq(catalog, &self.catalog) {
            self.sprites
                .get_or_init(|| {
                    std::sync::Arc::new(crate::sprites::SpritePalette::load(self, catalog))
                })
                .clone()
        } else {
            std::sync::Arc::new(crate::sprites::SpritePalette::load(self, catalog))
        }
    }

    fn decode(&self, selection: &AssetSelection) -> Result<PreviewAssets> {
        let tmp_name = selection.tmp_name.trim();
        let shp_name = selection.shp_name.trim();
        ensure!(
            !tmp_name.is_empty() && !shp_name.is_empty(),
            "请输入 TMP 和 SHP 文件名"
        );
        let (tmp_bytes, tmp_source) = self
            .manager
            .get_with_source_ref(tmp_name)
            .with_context(|| format!("找不到地块：{tmp_name}"))?;
        let tmp =
            TmpFile::from_bytes(tmp_bytes).with_context(|| format!("解析 {tmp_name} 失败"))?;
        let tile = tmp
            .tiles
            .get(selection.sub_tile)
            .and_then(Option::as_ref)
            .with_context(|| {
                format!(
                    "{tmp_name} 子块 {} 不存在或为空（共 {} 个槽位）",
                    selection.sub_tile,
                    tmp.tiles.len()
                )
            })?;
        let tile_rgba = tmp.tile_to_rgba(selection.sub_tile, &self.theater.iso_palette)?;
        let (shp_bytes, shp_source) = self
            .manager
            .get_with_source_ref(shp_name)
            .with_context(|| format!("找不到精灵：{shp_name}"))?;
        let shp =
            ShpFile::from_bytes(shp_bytes).with_context(|| format!("解析 {shp_name} 失败"))?;
        let frame = shp.frames.get(selection.frame).with_context(|| {
            format!(
                "{shp_name} 帧 {} 越界（共 {} 帧）",
                selection.frame,
                shp.frames.len()
            )
        })?;
        let sprite_rgba = shp.frame_to_rgba(selection.frame, &self.theater.unit_palette)?;
        let palette_source = |name: &str| -> Result<String> {
            self.manager
                .resolve_ref(name)
                .map(|r| r.source_archive.to_owned())
                .with_context(|| format!("找不到调色板：{name}"))
        };
        Ok(PreviewAssets {
            tile: DecodedImage {
                indices: Vec::new(),
                info: AssetInfo {
                    name: tmp_name.into(),
                    source: tmp_source.into(),
                    palette: "isotem.pal".into(),
                    palette_source: palette_source("isotem.pal")?,
                    index: selection.sub_tile,
                    count: tmp.tiles.len(),
                    offset: [tile.offset_x, tile.offset_y],
                    width: tile.pixel_width,
                    height: tile.pixel_height,
                },
                rgba: tile_rgba,
            },
            sprite: DecodedImage {
                indices: Vec::new(),
                info: AssetInfo {
                    name: shp_name.into(),
                    source: shp_source.into(),
                    palette: "unittem.pal".into(),
                    palette_source: palette_source("unittem.pal")?,
                    index: selection.frame,
                    count: shp.frames.len(),
                    offset: [i32::from(frame.frame_x), i32::from(frame.frame_y)],
                    width: u32::from(frame.frame_width),
                    height: u32::from(frame.frame_height),
                },
                rgba: sprite_rgba,
            },
            archive_count: self.manager.registered_archive_names().len(),
            tileset_slots: self.theater.lookup.len(),
        })
    }
}

pub struct LoadResult {
    pub generation: u64,
    pub result: Result<PreviewAssets>,
}

/// A single worker owns the archive mappings. Only decoded images cross into UI.
pub struct AssetLoader {
    requests: Sender<(u64, AssetSelection, bool)>,
    pub results: Receiver<LoadResult>,
}

impl AssetLoader {
    pub fn new(ctx: egui::Context) -> Result<Self> {
        let (requests, incoming) = mpsc::channel::<(u64, AssetSelection, bool)>();
        let (outgoing, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("asset-loader".into())
            .spawn(move || {
                let mut install: Option<Install> = None;
                while let Ok(mut request) = incoming.recv() {
                    // Superseded sliders/paths need no decoding or GPU upload.
                    for newer in incoming.try_iter() {
                        request = newer;
                    }
                    let (generation, selection, refresh) = request;
                    let result = (|| {
                        if refresh
                            || install
                                .as_ref()
                                .is_none_or(|i| i.directory != selection.directory)
                        {
                            // Drop the old mapping before opening a different installation.
                            install = None;
                            install = Some(Install::open(&selection.directory)?);
                        }
                        install
                            .as_ref()
                            .context("资源目录未加载")?
                            .decode(&selection)
                    })();
                    if outgoing.send(LoadResult { generation, result }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .context("无法启动资源加载线程")?;
        Ok(Self { requests, results })
    }

    pub fn request(&self, generation: u64, selection: AssetSelection, refresh: bool) -> Result<()> {
        if self
            .requests
            .send((generation, selection, refresh))
            .is_err()
        {
            bail!("资源加载线程已退出");
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "assets_tests.rs"]
pub(crate) mod tests;
