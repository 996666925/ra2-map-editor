//! Multiple source maps stay independent until an atomic composition is applied.
use crate::{
    document::{Cell, MapDocument, PreparedReplacement, Target},
    map_settings::{self, Footprints, Options},
    new_map::{MAX_MAP_SPAN, MIN_MAP_DIMENSION},
    region::{Clipboard, RegionLayers},
};
use anyhow::{Result, ensure};
use std::collections::BTreeSet;
use std::sync::Arc;

#[path = "stitch_storage.rs"]
mod storage;

#[derive(Clone)]
pub struct Piece {
    pub name: String,
    pub theater: String,
    pub clip: Arc<Clipboard>,
    pub offset: [i32; 2],
    pub max_level: u8,
    extent: [[i32; 2]; 2],
}

impl Piece {
    fn new(name: String, source: &MapDocument, clip: Clipboard, offset: [i32; 2]) -> Self {
        let width = source.base.header.width as i32;
        let height = source.base.header.height as i32;
        let max_level = clip
            .cells
            .iter()
            .filter_map(|c| c.terrain.map(|v| v.level))
            .max()
            .unwrap_or(0);
        Self {
            name,
            theater: source.base.header.theater.clone(),
            clip: Arc::new(clip),
            offset,
            max_level,
            extent: [[1 - width, width + 1], [width + 1, width + 2 * height + 1]],
        }
    }

    /// Exclusive flat projected frame, cached independently of the cell count.
    pub fn projected_bounds(&self) -> [[i32; 2]; 2] {
        let delta = [
            self.offset[0] - self.offset[1],
            self.offset[0] + self.offset[1],
        ];
        self.extent.map(|p| [p[0] + delta[0], p[1] + delta[1]])
    }
}

pub struct Workspace {
    baseline: MapDocument,
    identity: (u64, u64, u64),
    pub pieces: Vec<Piece>,
}

pub struct Composition {
    pub bytes: Vec<u8>,
    pub size: [u16; 2],
    /// Translation from the free layout to serialized map coordinates.
    pub shift: [i32; 2],
}

impl Workspace {
    /// A layer's art is immutable while its position changes. Decode it once
    /// using the destination's INI/resources, independently of the composition.
    pub fn layer_bytes(&self, index: usize) -> Result<Vec<u8>> {
        let piece = &self.pieces[index];
        let width = (piece.extent[1][0] - piece.extent[0][0]) / 2;
        let height = (piece.extent[1][1] - piece.extent[0][1]) / 2;
        let generated = crate::new_map::NewMapOptions {
            width: width as u16,
            height: height as u16,
            theater: self.baseline.base.header.theater.clone(),
            ..Default::default()
        }
        .bytes()?;
        let spatial = |section: &str| {
            matches!(
                section,
                "" | "Map"
                    | "Basic"
                    | "IsoMapPack5"
                    | "OverlayPack"
                    | "OverlayDataPack"
                    | "Preview"
                    | "PreviewPack"
                    | "Digest"
                    | "Structures"
                    | "Units"
                    | "Infantry"
                    | "Aircraft"
                    | "Terrain"
                    | "Smudge"
                    | "Waypoints"
                    | "CellTags"
                    | "Tubes"
                    | "TunnelPortalBackup"
                    | "TunnelPortalFacing"
                    | "TunnelReverse"
                    | storage::SECTION
            )
        };
        // New-map defaults contain Lighting too. Retain only spatial sections
        // so first-wins YR and last-wins Ares inherit exactly the current INI.
        let generated = crate::raw_ini::RawIni::parse(generated.into())?;
        let mut bytes = Vec::new();
        for line in &generated.lines {
            if spatial(&line.section) {
                bytes.extend_from_slice(&generated.bytes[line.bytes.clone()]);
            }
        }
        let current = self.baseline.snapshot().bytes()?;
        let raw = crate::raw_ini::RawIni::parse(current.into())?;
        bytes.extend_from_slice(b"\r\n");
        for line in &raw.lines {
            if !spatial(&line.section) {
                bytes.extend_from_slice(&raw.bytes[line.bytes.clone()]);
            }
        }
        let (source, _) = crate::source::SourceMap::parse_with_ares(
            piece.name.clone(),
            "layer".into(),
            bytes,
            self.baseline.base.reads_ares,
        )?;
        let mut doc = MapDocument::new(source.edit_base.map_err(anyhow::Error::msg)?);
        doc.resource_profile = self.baseline.resource_profile.clone();
        let waypoints: Vec<_> = doc.record_iter().map(|(id, _)| id).collect();
        for id in waypoints {
            doc.set(Target::Record(id), None)?;
        }
        let mut plan = piece.clip.plan(&doc, piece.clip.origin)?;
        for (_, record) in &mut plan.records {
            if record.section == "Terrain" {
                let at = record.cell().unwrap();
                record.coordinate_radix = 1000;
                *record = record.moved(at)?;
            }
        }
        plan.apply(&mut doc, true)?;
        doc.snapshot().bytes()
    }

    pub fn fork(&self) -> Self {
        Self {
            baseline: self.baseline.fork(),
            identity: self.identity,
            pieces: self.pieces.clone(),
        }
    }
    pub fn new(doc: &MapDocument, name: String) -> Result<Self> {
        let mut baseline = doc.fork();
        if doc.base.raw.entries(storage::SECTION).next().is_some() {
            baseline.replace_bytes(
                storage::without_layout(&crate::raw_ini::RawIni::parse(
                    doc.snapshot().bytes()?.into(),
                )?),
                "清理旧布局",
            )?;
        }
        // The original map retains its missions, keys, waypoints and bindings.
        let clip = Clipboard::capture_map(
            doc,
            RegionLayers {
                waypoints: true,
                cell_tags: true,
                ..Default::default()
            },
        )?;
        Ok(Self {
            baseline,
            identity: (doc.id, doc.state_id, doc.edit_revision()),
            pieces: vec![Piece::new(name, doc, clip, [0, 0])],
        })
    }

    pub fn matches(&self, doc: &MapDocument) -> bool {
        self.identity == (doc.id, doc.state_id, doc.edit_revision())
    }

    pub fn import(&mut self, name: String, source: &MapDocument) -> Result<()> {
        let clip = Clipboard::import_map(source, &self.baseline)?;
        // Import as an independent layer over the current map, centered in its
        // projected frame. Keep the untouched baseline for every recomposition.
        let center = (self.baseline.base.header.width as i32
            + self.baseline.base.header.height as i32
            - source.base.header.width as i32
            - source.base.header.height as i32)
            .div_euclid(2);
        let base = self.pieces[0].offset;
        self.pieces.push(Piece::new(
            name,
            source,
            clip,
            [base[0] + center, base[1] + center],
        ));
        Ok(())
    }

    /// Normalize free signed workspace coordinates into the smallest valid Size.
    fn geometry(&self) -> Result<(Options, [i32; 2])> {
        ensure!(!self.pieces.is_empty(), "没有可拼接的地图");
        let mut lo = [i32::MAX; 2];
        let mut hi = [i32::MIN; 2];
        for piece in &self.pieces {
            ensure!(
                piece
                    .offset
                    .iter()
                    .all(|v| (-i32::from(MAX_MAP_SPAN)..=i32::from(MAX_MAP_SPAN)).contains(v)),
                "地图偏移超出范围"
            );
            let [min, max] = piece.projected_bounds();
            lo[0] = lo[0].min(min[0]);
            lo[1] = lo[1].min(min[1]);
            hi[0] = hi[0].max(max[0] - 2);
            hi[1] = hi[1].max(max[1] - 1);
        }
        let du = -(lo[0] + hi[0]).div_euclid(2);
        let old_width = self.baseline.base.header.width as i32;
        let current = self.pieces[0].offset;
        let mut width = (hi[0] + du + 1)
            .max(1 - lo[0] - du)
            .max(i32::from(MIN_MAP_DIMENSION));
        // The existing structural migrator uses whole projected columns/rows.
        if (width - old_width - current[0] + current[1] - du).rem_euclid(2) != 0 {
            width += 1;
        }
        let mut dv = width + 1 - lo[1];
        if (dv - du).rem_euclid(2) != 0 {
            dv += 1;
        }
        let height = ((hi[1] + dv - width + 1) / 2).max(i32::from(MIN_MAP_DIMENSION));
        ensure!(
            width + height <= i32::from(MAX_MAP_SPAN),
            "拼接后的地图宽度＋高度超过 1024，请缩小间距或减少地图"
        );
        let shift = [(dv + du) / 2, (dv - du) / 2];
        let delta = [current[0] + shift[0], current[1] + shift[1]];
        let options = Options {
            width: width as u16,
            height: height as u16,
            offset: [
                (delta[0] - delta[1] + width - old_width) / 2,
                (delta[0] + delta[1] - width + old_width) / 2,
            ],
            local: [2, 4, width - 4, height - 10],
            fill_level: self.baseline.base.header.level as u8,
            delete_outside: false,
        };
        options.validate()?;
        ensure!(
            options.delta(self.baseline.base.header.width) == delta,
            "地图偏移无法对齐"
        );
        Ok((options, shift))
    }

    pub fn size(&self) -> Result<[u16; 2]> {
        let (options, _) = self.geometry()?;
        Ok([options.width, options.height])
    }

    pub fn compose(&self, footprints: &Footprints) -> Result<Composition> {
        let (options, shift) = self.geometry()?;
        let size = [options.width, options.height];
        let migrated = map_settings::plan(&self.baseline, options, footprints)?;
        ensure!(
            migrated.can_apply(),
            "当前地图存在无法迁移的空间引用，请先修正地图"
        );
        let (source, _) = crate::source::SourceMap::parse_with_ares(
            "stitch.yrm".into(),
            "stitch".into(),
            migrated.bytes,
            self.baseline.base.reads_ares,
        )?;
        let mut result = MapDocument::new(source.edit_base.map_err(anyhow::Error::msg)?);
        result.resource_profile = self.baseline.resource_profile.clone();
        for piece in self.pieces.iter().skip(1) {
            let at: [i32; 2] =
                [0, 1].map(|i| i32::from(piece.clip.origin[i]) + piece.offset[i] + shift[i]);
            ensure!(
                at.iter().all(|&n| (0..1024).contains(&n)),
                "导入地图超出格网"
            );
            let plan = piece.clip.plan(&result, [at[0] as u16, at[1] as u16])?;
            let cells: BTreeSet<Cell> = plan.cells.iter().map(|c| c.at).collect();
            // A later map replaces the entire covered area, including empty cells.
            let remove: Vec<_> = result
                .record_iter()
                .filter(|(_, r)| {
                    r.section != "Waypoints" && r.cell().is_some_and(|at| cells.contains(&at))
                })
                .map(|(id, r)| (id, r.clone()))
                .collect();
            let removed_units: BTreeSet<_> = remove
                .iter()
                .filter(|(_, r)| r.section == "Units")
                .map(|(_, r)| r.key.clone())
                .collect();
            for (id, _) in remove {
                result.set(Target::Record(id), None)?;
            }
            let unlink: Vec<_> = result
                .record_iter()
                .filter(|(_, r)| {
                    r.section == "Units"
                        && r.fields()
                            .get(11)
                            .is_some_and(|key| removed_units.contains(key))
                })
                .map(|(id, r)| Ok((id, r.patched(&[(11, "-1".into())])?)))
                .collect::<Result<_>>()?;
            for (id, r) in unlink {
                result.set(Target::Record(id), Some(crate::document::Value::Record(r)))?;
            }
            for (_, r) in &plan.records {
                if r.section == "Structures" {
                    let at = r.cell().unwrap();
                    for &(dx, dy) in footprints.get(&r.type_id()).into_iter().flatten() {
                        let x = i32::from(at[0]) + i32::from(dx);
                        let y = i32::from(at[1]) + i32::from(dy);
                        ensure!(x >= 0 && y >= 0 && x < 1024 && y < 1024, "建筑占地超出格网");
                        result.check_cell([x as u16, y as u16])?;
                    }
                }
            }
            plan.apply(&mut result, true)?;
        }
        Ok(Composition {
            bytes: self.persist(result.snapshot().bytes()?, shift)?,
            size,
            shift,
        })
    }

    pub fn apply(&mut self, doc: &mut MapDocument, composition: Composition) -> Result<bool> {
        ensure!(self.matches(doc), "当前地图已经改变，请重新进入多地图管理");
        let changed = doc.replace_bytes(composition.bytes, "地图拼接")?;
        self.identity = (doc.id, doc.state_id, doc.edit_revision());
        Ok(changed)
    }

    pub fn apply_prepared(
        &mut self,
        doc: &mut MapDocument,
        prepared: PreparedReplacement,
    ) -> Result<bool> {
        ensure!(self.matches(doc), "当前地图已经改变，请重新进入多地图管理");
        let changed = doc.replace_prepared(prepared, "地图拼接")?;
        self.identity = (doc.id, doc.state_id, doc.edit_revision());
        Ok(changed)
    }
}

#[cfg(test)]
#[path = "stitch_tests.rs"]
mod tests;
