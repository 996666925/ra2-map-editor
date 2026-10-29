//! Derived terrain scenes, diagnostics, spatial indexing and pixel picking.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::{assets::Install, catalog};
use ra2_map_document::source::MAX_SOURCE_BYTES;
pub use ra2_map_document::source::SourceMap;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::Path,
    sync::Arc,
    time::Instant,
};
use vera20k_core::map::{
    playfield::{self, PlayfieldBounds},
    resolved_terrain::ResolvedTerrainGrid,
    terrain::{self, TerrainCell},
    theater::{self, TileImage, TileKey},
};

#[derive(Clone, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub cell: Option<[u16; 2]>,
}

#[derive(Clone)]
pub struct MapScene {
    pub(crate) resolved: Option<Arc<ResolvedTerrainGrid>>,
    pub revision: Arc<ra2_map_assets::profile::Revision>,
    pub mission_resources: Arc<ra2_map_assets::mission_resources::MissionResources>,
    pub mission_definitions: Arc<ra2_map_document::mission::ExternalDefinitions>,
    pub visual: Arc<crate::visual::VisualScene>,
    pub source: SourceMap,
    pub cells: Vec<TerrainCell>,
    pub images: Arc<HashMap<TileKey, TileImage>>,
    pub bounds: egui::Rect,
    pub diagnostics: Vec<Diagnostic>,
    pub elapsed_ms: u128,
    pub playfield: PlayfieldBounds,
    pub catalog: Arc<catalog::Catalog>,
    pub brushes: Vec<catalog::TerrainBrush>,
    pub palette: Arc<catalog::TerrainPalette>,
    pub sprites: Arc<ra2_map_assets::sprites::SpritePalette>,
    buckets: HashMap<(i32, i32), Vec<usize>>,
}
impl MapScene {
    pub fn key(cell: &TerrainCell) -> TileKey {
        TileKey {
            tile_id: cell.tile_id,
            sub_tile: cell.sub_tile,
            variant: cell.variant,
        }
    }
    pub fn image(&self, cell: &TerrainCell) -> Option<&TileImage> {
        self.images.get(&Self::key(cell))
    }
    pub fn cell_rect(&self, cell: &TerrainCell) -> Option<egui::Rect> {
        let image = self.image(cell)?;
        Some(egui::Rect::from_min_size(
            egui::pos2(
                cell.screen_x + image.offset_x as f32,
                cell.screen_y + image.offset_y as f32,
            ),
            egui::vec2(image.width as f32, image.height as f32),
        ))
    }
    /// Same alpha, nearest texel and TMP depth admission as the GPU terrain pass.
    /// This selects the visible terrain pixel, including the face of a cliff.
    pub fn pick(&self, world: egui::Pos2) -> Option<usize> {
        let mut winner = None;
        let mut depth = u16::MAX;
        let bucket = (
            (world.x / 128.).floor() as i32,
            (world.y / 128.).floor() as i32,
        );
        for &index in self.buckets.get(&bucket)? {
            let cell = &self.cells[index];
            let Some(rect) = self.cell_rect(cell) else {
                continue;
            };
            if world.x < rect.min.x
                || world.y < rect.min.y
                || world.x >= rect.max.x
                || world.y >= rect.max.y
            {
                continue;
            }
            let image = self.image(cell).unwrap();
            let x = (world.x - rect.min.x).floor() as usize;
            let y = (world.y - rect.min.y).floor() as usize;
            let pixel = y * image.width as usize + x;
            if image.rgba[pixel * 4 + 3] == 0 {
                continue;
            }
            let z = pixel_depth(cell, image.depth[pixel]);
            if z <= depth {
                depth = z;
                winner = Some(index);
            }
        }
        winner
    }
    fn index_pixels(&mut self) {
        self.buckets.clear();
        for (index, cell) in self.cells.iter().enumerate() {
            let Some(rect) = self.cell_rect(cell) else {
                continue;
            };
            for by in (rect.min.y / 128.).floor() as i32..=(rect.max.y / 128.).floor() as i32 {
                for bx in (rect.min.x / 128.).floor() as i32..=(rect.max.x / 128.).floor() as i32 {
                    self.buckets.entry((bx, by)).or_default().push(index);
                }
            }
        }
    }
    pub fn is_playable(&self, cell: &TerrainCell) -> bool {
        self.playfield
            .contains_geometry_packed(i32::from(cell.rx), i32::from(cell.ry))
    }
    pub fn pick_checks(&self) -> Result<serde_json::Value> {
        let mut probes = Vec::new();
        // Four extrema of Size plus the highest visible cell. Each probe scans
        // the cell's opaque texels to avoid testing a point hidden by a cliff.
        for axis in 0..6 {
            let candidate = self
                .cells
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    self.image(c)
                        .is_some_and(|image| axis != 5 || image.height > 30)
                })
                .max_by_key(|(_, c)| match axis {
                    0 => -(i32::from(c.rx) + i32::from(c.ry)),
                    1 => i32::from(c.rx) + i32::from(c.ry),
                    2 => i32::from(c.rx) - i32::from(c.ry),
                    3 => i32::from(c.ry) - i32::from(c.rx),
                    4 => i32::from(c.z as i8),
                    _ => self.image(c).unwrap().height as i32,
                });
            let Some(candidate) = candidate else {
                ensure!(axis == 5, "没有可拾取地形：P1 地形烟测需要 TEMPERATE 地图");
                continue;
            };
            let (index, cell) = candidate;
            let rect = self.cell_rect(cell).unwrap();
            let image = self.image(cell).unwrap();
            let mut found = None;
            'pixels: for y in 0..image.height {
                for x in 0..image.width {
                    if image.rgba[((y * image.width + x) * 4 + 3) as usize] == 0 {
                        continue;
                    }
                    let world = rect.min + egui::vec2(x as f32 + 0.5, y as f32 + 0.5);
                    if self.pick(world) == Some(index) {
                        found = Some(world);
                        break 'pixels;
                    }
                }
            }
            let point = found.context("边缘/高度样本完全不可见")?;
            probes.push(serde_json::json!({"probe": (["north","south","east","west","highest","cliff"][axis]), "cell":[cell.rx,cell.ry], "level":cell.z,"world":[point.x,point.y]}));
        }
        ensure!(
            self.pick(self.bounds.min - egui::vec2(100., 100.))
                .is_none(),
            "地图外拾取失败"
        );
        Ok(serde_json::json!({"visible_pixel_probes":probes,"outside":true}))
    }
}

/// Native base = -diamond_top - 30 - 15*level, plus TMP per-pixel Z.
/// A constant camera origin cancels from comparisons in this terrain-only pass.
pub fn pixel_depth(cell: &TerrainCell, byte: u8) -> u16 {
    (32768 - cell.screen_y as i32 - 30 - i32::from(cell.z as i8) * 15 + i32::from(byte)) as u16
}

pub fn load_scene(install: &mut Install, name: &str) -> Result<MapScene> {
    load_scene_controlled(install, name, &|| false)
}
pub fn load_scene_controlled(
    install: &mut Install,
    name: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<MapScene> {
    crate::work::checkpoint(cancelled)?;
    let started = Instant::now();
    let path = Path::new(name);
    let (bytes, origin) = if path.is_file() {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((MAX_SOURCE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        (bytes, path.display().to_string())
    } else {
        // Only a bare archive name may fall back to MIX lookup. Typos in a
        // filesystem path should not silently load an unrelated embedded map.
        ensure!(path.components().count() == 1, "地图文件不存在：{name}");
        let (bytes, source) = install
            .manager
            .get_with_source_ref(name)
            .with_context(|| format!("找不到地图：{name}"))?;
        ensure!(bytes.len() <= MAX_SOURCE_BYTES, "地图超过 32 MiB");
        (bytes.to_vec(), source.to_owned())
    };
    load_scene_bytes_controlled(
        install,
        name,
        origin,
        bytes,
        started,
        Default::default(),
        cancelled,
    )
}
pub fn load_scene_bytes(
    install: &mut Install,
    name: &str,
    origin: String,
    bytes: Vec<u8>,
    started: Instant,
) -> Result<MapScene> {
    load_scene_bytes_with_profile(
        install,
        name,
        origin,
        bytes,
        started,
        ra2_map_document::lighting::Profile::Normal,
    )
}
pub fn load_scene_bytes_with_profile(
    install: &mut Install,
    name: &str,
    origin: String,
    bytes: Vec<u8>,
    started: Instant,
    profile: ra2_map_document::lighting::Profile,
) -> Result<MapScene> {
    load_scene_bytes_controlled(install, name, origin, bytes, started, profile, &|| false)
}
pub fn load_scene_bytes_controlled(
    install: &mut Install,
    name: &str,
    origin: String,
    bytes: Vec<u8>,
    started: Instant,
    profile: ra2_map_document::lighting::Profile,
    cancelled: &dyn Fn() -> bool,
) -> Result<MapScene> {
    crate::work::checkpoint(cancelled)?;
    let (source, map) = SourceMap::parse(name.into(), origin, bytes)?;
    let mut diagnostics = vec![Diagnostic { code:"EDITOR_PREVIEW", message:"静态编辑预览：SHP/VXL 与地形共享像素深度，支持 House 换色、基础地图光照和建筑阴影；尚未覆盖损伤动画、局部光源、全部桥梁及对象阴影。地形工具支持局部 LAT、连续坡面、标准陆地悬崖、高台改形、坡道及高低桥梁联动；自动道路布线、水岸与水悬崖生成尚未实现。".into(),cell:None }];
    let playfield = PlayfieldBounds::from_map_header(&map.header);
    if let Err(error) = install.activate_theater(&map.header.theater) {
        diagnostics.push(Diagnostic {
            code: "UNSUPPORTED_THEATER",
            message: format!(
                "无法加载 {} 环境资源：{error:#}。可查看地图头并导出原文。",
                map.header.theater
            ),
            cell: None,
        });
        return Ok(MapScene {
            resolved: None,
            revision: install.revision.clone(),
            mission_resources: install.mission_resources(),
            mission_definitions: Arc::new(crate::mission_catalog::load(install)),
            visual: Default::default(),
            source,
            cells: vec![],
            images: Default::default(),
            bounds: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(60., 30.)),
            diagnostics,
            elapsed_ms: started.elapsed().as_millis(),
            playfield,
            buckets: HashMap::new(),
            catalog: install.catalog.clone(),
            brushes: vec![],
            palette: Default::default(),
            sprites: Default::default(),
        });
    }
    crate::work::checkpoint(cancelled)?;
    let catalog = install.catalog.with_map(source.bytes());
    let sprites = install.sprite_palette(&catalog);
    for cell in &map.cells {
        if !playfield::size_diamond_contains(
            map.header.width as i32,
            map.header.height as i32,
            (cell.rx as i16, cell.ry as i16),
        ) && diagnostics
            .iter()
            .filter(|d| d.code == "OUTSIDE_SIZE")
            .count()
            < 16
        {
            diagnostics.push(Diagnostic {
                code: "OUTSIDE_SIZE",
                message: "原文格子在 Size 菱形之外，浏览器不显示；保存仍保留。".into(),
                cell: Some([cell.rx, cell.ry]),
            });
        }
    }
    let mut selector_cache =
        vera20k_core::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut rng = vera20k_core::sim::rng::SimRng::new(0);
    let mut random = || rng.next_u32();
    let mut fill = |low, _high| low;
    let mut selector = selector_cache.begin_load(&mut random);
    let resolved = ResolvedTerrainGrid::build_with_variant_selector(
        &map,
        Some(&install.theater),
        Some(&install.manager),
        None,
        None,
        None,
        false,
        0,
        &mut fill,
        &mut selector,
    );
    crate::work::checkpoint(cancelled)?;
    let mut grid = terrain::build_terrain_grid_from_resolved(&resolved, None);
    grid.cells.retain(|c| {
        playfield::size_diamond_contains(
            map.header.width as i32,
            map.header.height as i32,
            (c.rx as i16, c.ry as i16),
        )
    });
    let mut needed: HashSet<_> = grid.cells.iter().map(MapScene::key).collect();
    needed.extend(install.catalog.defaults.iter().copied());
    // Diagnose authored choices before core's ClearTile fallback hides them.
    // Raw -1 must cross LastTilesInSet translation before sentinel admission.
    let mut authored = HashMap::new();
    let mut invalid_ids = HashSet::new();
    for cell in &map.cells {
        let tile = install
            .theater
            .lookup
            .translate_legacy_map_tile_index(cell.tile_index);
        if tile == -1 || tile == 0xFFFF {
            continue;
        }
        if tile < 0 || tile as usize >= install.theater.lookup.len() {
            if invalid_ids.len() < 16 && invalid_ids.insert(tile) {
                diagnostics.push(Diagnostic {code:"INVALID_TILE_ID",message:format!("原文地形索引 {} 翻译为 {tile}，超出 theater 范围；显示 core 的 ClearTile 替代。",cell.tile_index),cell:Some([cell.rx,cell.ry])});
            }
            continue;
        }
        authored
            .entry(TileKey {
                tile_id: tile as u16,
                sub_tile: cell.sub_tile,
                variant: 0,
            })
            .or_insert([cell.rx, cell.ry]);
    }
    let all_needed = needed
        .union(&authored.keys().copied().collect())
        .copied()
        .collect();
    let mut images = theater::load_tile_images(
        &install.manager,
        &install.theater.lookup,
        &install.theater.iso_palette,
        &all_needed,
    );
    for key in all_needed
        .iter()
        .filter(|key| !images.contains_key(key))
        .take(64)
    {
        diagnostics.push(Diagnostic {
            code: "MISSING_TILE",
            message: format!(
                "未解码地形 {}:{} variant={} ({})",
                key.tile_id,
                key.sub_tile,
                key.variant,
                install
                    .theater
                    .lookup
                    .filename(i32::from(key.tile_id))
                    .unwrap_or("无文件名")
            ),
            cell: authored.get(key).copied().or_else(|| {
                grid.cells
                    .iter()
                    .find(|c| MapScene::key(c) == *key)
                    .map(|c| [c.rx, c.ry])
            }),
        });
    }
    images.retain(|key, _| needed.contains(key));
    let mut bounds = egui::Rect::NOTHING;
    for c in &grid.cells {
        if let Some(image) = images.get(&MapScene::key(c)) {
            bounds = bounds.union(egui::Rect::from_min_size(
                egui::pos2(
                    c.screen_x + image.offset_x as f32,
                    c.screen_y + image.offset_y as f32,
                ),
                egui::vec2(image.width as f32, image.height as f32),
            ));
        }
    }
    if !bounds.is_finite() {
        diagnostics.push(Diagnostic {
            code: "NO_RENDERABLE_TERRAIN",
            message: "地图没有可显示的 TMP 地形，请检查资源目录；仍可导出原文。".into(),
            cell: None,
        });
        bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(60., 30.));
    }
    // The material library belongs to the installation, not the current map's
    // atlas. A blank map must also offer roads, cliffs and all other subtiles.
    let palette = install.terrain_palette();
    let brushes = palette.brushes.clone();
    let mut scene = MapScene {
        resolved: None,
        revision: install.revision.clone(),
        mission_resources: install.mission_resources(),
        mission_definitions: Arc::new(crate::mission_catalog::load(install)),
        visual: Default::default(),
        source,
        cells: grid.cells,
        images: Arc::new(images),
        bounds,
        diagnostics,
        elapsed_ms: started.elapsed().as_millis(),
        playfield,
        buckets: HashMap::new(),
        catalog,
        sprites,
        brushes,
        palette,
    };
    scene.index_pixels();
    scene.visual = Arc::new(crate::visual::VisualScene::build_cached(
        &scene, install, &resolved, profile, None, cancelled,
    )?);
    scene.resolved = Some(Arc::new(resolved));
    Ok(scene)
}

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
