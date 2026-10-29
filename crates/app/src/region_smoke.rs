//! Real installation checks for mixed regions and typed object edits.
use super::*;
use std::path::Path;

fn click(editor: &mut Editor, scene: &MapScene, at: Cell) -> Result<()> {
    editor.pointer(scene, Some(at), true, false, true, None);
    ensure!(!editor.status.starts_with("未修改"), "{}", editor.status);
    Ok(())
}
fn undo_redo(editor: &mut Editor, scene: &MapScene, before: Vec<u8>) -> Result<()> {
    let after = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    ensure!(after != before, "操作未产生修改");
    editor.history(false, scene);
    ensure!(
        editor.doc.as_ref().unwrap().snapshot().bytes()? == before,
        "撤销不一致"
    );
    editor.history(true, scene);
    ensure!(
        editor.doc.as_ref().unwrap().snapshot().bytes()? == after,
        "重做不一致"
    );
    Ok(())
}
impl Editor {
    pub(crate) fn check_regions(
        &mut self,
        scene: &MapScene,
        output: &Path,
    ) -> Result<serde_json::Value> {
        self.allow_overlap = false;
        self.tool = Tool::Place;
        self.owner = "Neutral".into();
        for (section, type_id, at) in [
            ("Structures", "GAPOWR", [60, 90]),
            ("Units", "HTNK", [63, 90]),
            ("Infantry", "E1", [64, 90]),
        ] {
            self.section = section.into();
            self.type_id = type_id.into();
            click(self, scene, at)?;
            ensure!(
                self.doc
                    .as_ref()
                    .unwrap()
                    .records()
                    .values()
                    .any(|r| r.section == section && r.cell() == Some(at)),
                "{section}: {}",
                self.status
            );
        }
        self.infantry_subcell = 1;
        click(self, scene, [64, 90])?;
        ensure!(
            self.doc
                .as_ref()
                .unwrap()
                .records()
                .values()
                .filter(|r| r.section == "Infantry" && r.cell() == Some([64, 90]))
                .count()
                == 2,
            "步兵子格放置失败"
        );
        self.infantry_subcell = 0;
        let before = self.doc.as_ref().unwrap().snapshot().bytes()?;
        self.pointer(scene, Some([64, 90]), true, false, true, None);
        ensure!(
            self.doc.as_ref().unwrap().snapshot().bytes()? == before,
            "同子格冲突未阻止"
        );
        for (section, at) in [("Terrain", [65, 90]), ("Smudge", [66, 90])] {
            self.section = section.into();
            self.type_id = scene.catalog.object_types[section]
                .iter()
                .find(|id| {
                    scene
                        .sprites
                        .objects
                        .contains_key(&(section.into(), (*id).clone()))
                })
                .context("没有环境对象")?
                .clone();
            click(self, scene, at)?;
        }
        let id = self
            .doc
            .as_ref()
            .unwrap()
            .records()
            .iter()
            .find_map(|(&id, r)| (r.section == "Units" && r.cell() == Some([63, 90])).then_some(id))
            .context("no tank")?;
        let doc = self.doc.as_mut().unwrap();
        let mut r = doc.records()[&id].clone();
        r.value.extend_from_slice(b",\xd6\xd0,extension");
        doc.set(Target::Record(id), Some(Value::Record(r.clone())))?;
        doc.commit("opaque fixture");
        self.cache(scene);
        self.select(Some(id));
        self.property_extra.insert(6, "Hunt".into());
        self.property_extra.insert(8, "100".into());
        self.apply_object_properties(scene, &r, id)?;
        ensure!(
            self.doc.as_ref().unwrap().records()[&id]
                .value
                .ends_with(b",\xd6\xd0,extension"),
            "属性编辑破坏尾字段"
        );
        self.tool = Tool::Clone;
        click(self, scene, [63, 93])?;
        let clone = self.selected.context("克隆未选中新对象")?;
        ensure!(
            clone != id
                && self.doc.as_ref().unwrap().records()[&clone]
                    .value
                    .ends_with(b",\xd6\xd0,extension"),
            "克隆属性丢失"
        );
        self.tool = Tool::Ore;
        self.radius = 1;
        click(self, scene, [67, 91])?;
        let road = scene
            .brushes
            .iter()
            .position(|b| {
                b.key.variant == 0
                    && b.label.to_ascii_lowercase().contains("road")
                    && scene
                        .palette
                        .templates
                        .get(&(b.key.tile_id, 0))
                        .is_some_and(|t| t.width <= 5 && t.height <= 3 && t.cells.len() > 1)
            })
            .context("没有可用道路")?;
        self.tool = Tool::Terrain;
        self.terrain_mode = TerrainMode::Template;
        self.brush = road;
        click(self, scene, [60, 92])?;
        let doc = self.doc.as_mut().unwrap();
        ra2_map_document::mission::create(
            doc,
            ra2_map_document::mission::Kind::Trigger,
            "REGION_TRIGGER",
            "Neutral",
            None,
        )?;
        let tag =
            ra2_map_document::mission::ids(doc, ra2_map_document::mission::Kind::Tag)[0].clone();
        ra2_map_document::mission::bind_cell(doc, [63, 90], &tag)?;
        doc.commit("region cell tag fixture");
        self.tool = Tool::Region;
        self.pointer(scene, Some([60, 90]), true, true, false, None);
        self.pointer(scene, Some([69, 94]), false, false, true, None);
        ensure!(self.region.cells.len() == 50, "框选末帧漏格");
        self.region_action(scene, Action::Copy);
        let clip = self.region.clipboard.as_ref().context("复制失败")?.clone();
        ensure!(clip.records.len() >= 7, "混合对象未复制完整");
        ensure!(
            clip.cells
                .iter()
                .any(|c| c.cell_tag.as_deref() == Some(tag.as_bytes())),
            "未复制格子标签"
        );
        let before = self.doc.as_ref().unwrap().snapshot().bytes()?;
        self.region_action(scene, Action::Paste);
        click(self, scene, [80, 110])?;
        let doc = self.doc.as_ref().unwrap();
        ensure!(
            doc.ini_text("CellTags", "110083") == tag && doc.ini_text("CellTags", "90063") == tag,
            "共享 Tag 复制失败"
        );
        for (_, r) in &clip.records {
            let at = r.cell().unwrap();
            let target = [at[0] + 20, at[1] + 20];
            let moved = r.moved(target)?;
            ensure!(
                doc.records().values().any(|r| r.section == moved.section
                    && r.value == moved.value
                    && r.cell() == Some(target)),
                "区域粘贴丢失 {} 属性",
                r.section
            );
        }
        ensure!(
            doc.overlay([87, 111]) == doc.overlay([67, 91]),
            "资源未复制"
        );
        undo_redo(self, scene, before)?;
        self.tool = Tool::RegionMove;
        let before = self.doc.as_ref().unwrap().snapshot().bytes()?;
        click(self, scene, [61, 90])?;
        ensure!(
            self.doc.as_ref().unwrap().records()[&id].cell() == Some([64, 90]),
            "区域移动未保留记录身份"
        );
        ensure!(
            self.doc.as_ref().unwrap().ini_text("CellTags", "90064") == tag
                && self
                    .doc
                    .as_ref()
                    .unwrap()
                    .ini_value("CellTags", "90063")
                    .is_none(),
            "重叠移动留下格子标签"
        );
        undo_redo(self, scene, before)?;
        self.history(false, scene); // Keep the unmoved source for the visual preview.
        self.region.cells = clip.cells.iter().map(|c| c.at).collect();
        self.tool = Tool::Region;
        self.region_action(scene, Action::Cut);
        ensure!(
            self.doc
                .as_ref()
                .unwrap()
                .ini_value("CellTags", "90063")
                .is_none(),
            "剪切未移除 CellTag"
        );
        ensure!(
            !self.doc.as_ref().unwrap().records().contains_key(&id),
            "剪切未移除对象"
        );
        self.history(false, scene);
        ensure!(
            self.doc.as_ref().unwrap().ini_text("CellTags", "90063") == tag,
            "剪切撤销未恢复 CellTag"
        );
        ensure!(
            self.doc.as_ref().unwrap().records().contains_key(&id),
            "剪切撤销未恢复对象"
        );
        let before = self.doc.as_ref().unwrap().snapshot().bytes()?;
        self.tool = Tool::Paste;
        self.pointer(scene, Some([1, 1]), true, false, true, None);
        ensure!(
            self.doc.as_ref().unwrap().snapshot().bytes()? == before,
            "越界粘贴部分写入"
        );
        let path = output.join("region-objects.map");
        std::fs::write(&path, &before)?;
        let (source, _) =
            crate::map::SourceMap::parse("reopened.map".into(), "region smoke".into(), before)?;
        let reopened = MapDocument::new(source.edit_base.map_err(anyhow::Error::msg)?);
        let records = reopened.records();
        ensure!(
            reopened.ini_text("CellTags", "110083") == tag
                && reopened.ini_text("CellTags", "90063") == tag,
            "重开丢失 CellTag"
        );
        ensure!(
            records
                .values()
                .filter(|r| r.section == "Units" && r.value.ends_with(b",\xd6\xd0,extension"))
                .count()
                >= 4,
            "重开丢失克隆或不透明属性"
        );
        let identities: BTreeSet<_> = records.values().map(|r| (&r.section, &r.key)).collect();
        ensure!(identities.len() == records.len(), "记录编号重复");
        Ok(
            serde_json::json!({"passed":true,"selection_cells":50,"source_objects":clip.records.len(),"mixed_layers":true,"cell_tags_shared_copy_move_cut_undo_reload":true,"infantry_subcells":true,"object_clone_preserves_tail":true,"property_edit_preserves_tail":true,"copy_paste_undo_redo":true,"overlapping_move":true,"cut_undo":true,"boundary_atomic":true,"unique_keys_on_reload":true,"map":path}),
        )
    }
    pub(crate) fn region_preview_is_valid(&self) -> bool {
        self.region
            .preview
            .as_ref()
            .is_some_and(|p| p.error.is_none() && !p.ground.is_empty() && !p.records.is_empty())
    }
    pub(crate) fn prepare_region_smoke_preview(&mut self, scene: &MapScene) {
        self.region.cells = (60..=69)
            .flat_map(|x| (90..=94).map(move |y| [x, y]))
            .collect();
        self.region_action(scene, Action::Copy);
        self.tool = Tool::Paste;
        self.show_library(1, "Structures", "");
    }
}
