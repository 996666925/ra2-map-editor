//! Parsed object geometry shared by canvas interaction and the navigator.
use super::*;

pub(crate) struct Marker {
    pub id: u64,
    pub origin: egui::Pos2,
    pub radar_origin: egui::Pos2,
    /// Counts before this record, split by object/waypoint visibility.
    pub slots: [usize; 2],
    pub waypoint: bool,
    pub label: String,
    pub waypoint_label: Option<String>,
    pub radar_radius: f32,
    pub image: Option<Arc<ra2_map_assets::assets::DecodedImage>>,
    pub image_rect: Option<egui::Rect>,
}
#[derive(Default)]
pub(crate) struct MarkerCache {
    pub markers: Vec<Marker>,
    pub owners: HashMap<u64, u64>,
}
impl MarkerCache {
    pub fn new(
        scene: &MapScene,
        doc: Option<&MapDocument>,
        records: &BTreeMap<u64, Record>,
    ) -> Self {
        let Some(doc) = doc else {
            return Self::default();
        };
        let mut sorted: Vec<_> = records.iter().collect();
        // cell() parses CSV fields. Evaluate each key once, not twice per
        // comparison, and never repeat the sort during pan/zoom/hover frames.
        sorted.sort_by_cached_key(|(_, r)| {
            (
                r.section != "Smudge",
                r.cell().map_or(0, |at| u32::from(at[0]) + u32::from(at[1])),
            )
        });
        let mut stacked = HashMap::<Cell, [usize; 2]>::new();
        let markers = sorted
            .into_iter()
            .filter_map(|(&id, record)| {
                let at = record.cell()?;
                let ground = terrain_tools::value(doc, scene, at);
                let origin = egui::pos2(
                    (f32::from(at[0]) - f32::from(at[1])) * 30.,
                    (f32::from(at[0]) + f32::from(at[1])) * 15. + 30.
                        - f32::from(ground.level as i8) * 15.,
                );
                let waypoint = record.section == "Waypoints";
                let (rx, ry) =
                    vera20k_core::map::terrain::iso_to_screen(at[0], at[1], ground.level);
                let slot = stacked.entry(at).or_default();
                let slots = *slot;
                slot[usize::from(waypoint)] += 1;
                let label = record.type_id();
                let fields = record.fields();
                let facing = fields
                    .get(if record.section == "Infantry" { 7 } else { 5 })
                    .and_then(|s| s.parse::<u8>().ok())
                    .unwrap_or(0);
                let appearance = scene
                    .sprites
                    .objects
                    .get(&(record.section.clone(), label.clone()));
                let image = appearance.map(|a| a.image(facing).clone());
                let image_rect = appearance.zip(image.as_ref()).map(|(appearance, image)| {
                    let z = terrain_tools::object_world_z(
                        scene,
                        at,
                        ground,
                        ra2_map_scene::visual::object_on_bridge(record),
                    );
                    let point = ra2_map_scene::visual::object_anchor(record, appearance, z);
                    let position = ra2_map_scene::visual::sprite_position(point, image);
                    egui::Rect::from_min_size(
                        position,
                        egui::vec2(image.info.width as f32, image.info.height as f32),
                    )
                });
                let label = if waypoint {
                    label
                } else {
                    scene.catalog.object_label(&label).into_owned()
                };
                Some(Marker {
                    id,
                    origin,
                    radar_origin: egui::pos2(rx + 30., ry + 15.),
                    slots,
                    waypoint,
                    label,
                    waypoint_label: (waypoint && record.key.parse::<u32>().is_ok_and(|n| n < 30))
                        .then(|| record.key.clone()),
                    radar_radius: match record.section.as_str() {
                        "Structures" => 1.8,
                        "Units" | "Infantry" | "Aircraft" => 1.1,
                        _ => 0.,
                    },
                    image,
                    image_rect,
                })
            })
            .collect();
        let identities: HashMap<_, _> = records
            .iter()
            .map(|(&id, r)| ((r.section.as_str(), r.key.as_str()), id))
            .collect();
        let owners = scene
            .visual
            .owners
            .iter()
            .filter_map(|(&id, (section, key))| {
                identities
                    .get(&(section.as_str(), key.as_str()))
                    .map(|&current| (id, current))
            })
            .collect();
        Self { markers, owners }
    }
}
