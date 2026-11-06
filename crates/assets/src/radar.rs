//! Immutable inputs for the engine's traditional terrain-preview callbacks.
use ra2_map_voxel::{preview_mix as mix, preview_pack as pack, preview_unpack as unpack};
use std::collections::HashMap;
use vera20k_core::{
    assets::{asset_manager::AssetManager, shp_file::ShpFile},
    rules::{
        art_data,
        effect_asset_catalog::load_overlay_shape,
        overlay_types::{OverlayTypeRegistry, overlay_radar_channel_order},
        ruleset::RuleSet,
        terrain_object_type::TerrainObjectType,
    },
};

pub type Pair = ([u8; 3], [u8; 3]);
pub struct Palette {
    pub overlays: OverlayTypeRegistry,
    pub headers: HashMap<(u8, u8), [u8; 3]>,
    pub terrain: HashMap<String, TerrainObjectType>,
}
impl Default for Palette {
    fn default() -> Self {
        Self {
            overlays: OverlayTypeRegistry::empty(),
            headers: HashMap::new(),
            terrain: HashMap::new(),
        }
    }
}
impl Palette {
    pub fn load(rules: &RuleSet, manager: &AssetManager, theater: &str, extension: &str) -> Self {
        let overlays = rules.overlay_registry().clone();
        let mut headers = HashMap::new();
        for id in 0..=u8::MAX {
            let Some(name) = overlays.name(id) else {
                continue;
            };
            let shp = load_overlay_shape(manager, &overlays, name, extension, theater)
                .map(|(_, shp)| shp)
                .or_else(|| {
                    let anim = overlays.flags(id)?.cell_anim.as_deref()?;
                    let image = rules.art().resolve_anim_image_id(anim);
                    art_data::effect_shp_candidates(
                        art_data::EffectImageKind::Animation,
                        Some(rules.art()),
                        anim,
                        &image,
                        extension,
                        theater,
                    )
                    .iter()
                    .find_map(|name| ShpFile::from_bytes(manager.get_ref(name)?).ok())
                });
            if let Some(shp) = shp {
                for (frame, header) in shp.frames.iter().take(256).enumerate() {
                    headers.insert(
                        (id, frame as u8),
                        overlay_radar_channel_order(id, header.radar_color),
                    );
                }
            }
        }
        Self {
            overlays,
            headers,
            terrain: rules.terrain_object_types.clone(),
        }
    }

    /// GenerateTerrainPreview6413C9..6416EF: SHP-header/type colour and density,
    /// using the engine's actual packed-surface mixer rather than editor dots.
    pub fn overlay(&self, id: u8, stage: u8, ground: Pair) -> Option<Pair> {
        let flags = self.overlays.flags(id)?;
        let declared = pack(flags.radar_color.unwrap_or([0; 3]));
        let header = pack(self.headers.get(&(id, stage)).copied().unwrap_or([0; 3]));
        let (source, amount) = if flags.tiberium {
            (
                if declared > 5 { declared } else { header },
                if stage < 2 {
                    25
                } else if stage < 5 {
                    50
                } else {
                    75
                },
            )
        } else {
            let source = if header > 5 { header } else { declared };
            if source == 0 {
                return None;
            }
            (source, 100)
        };
        if amount == 100 {
            return Some((unpack(source), unpack(source)));
        }
        let first = mix(source, pack(ground.1), amount);
        Some((
            unpack(mix(source, 0, amount)),
            unpack(mix(source, first, amount)),
        ))
    }

    /// Terrain head branch6415E1..641689, after the engine binds type colours.
    pub fn terrain_color(rgb: [u8; 3], ground: Pair) -> Pair {
        let source = pack(rgb);
        (
            unpack(mix(source, pack(ground.0), 75)),
            unpack(mix(source, pack(ground.1), 75)),
        )
    }
    pub fn quantize(pair: Pair) -> Pair {
        (unpack(pack(pair.0)), unpack(pack(pair.1)))
    }
}
