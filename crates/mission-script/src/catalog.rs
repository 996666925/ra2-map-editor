//! Authoring choices and native registration identities. Display order is never an operand.
use anyhow::{Context, Result, ensure};
use ra2_map_document::{
    document::MapDocument,
    mission::{self, Kind},
    startup_owner,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// SDK reference categories; each collection keeps its own native identity space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    Building,
    Techno,
    Country,
    Sound,
    Eva,
    Theme,
    Movie,
    Animation,
    Weapon,
    SuperWeapon,
    Warhead,
    ParticleSystem,
}
impl ResourceKind {
    pub const ALL: [Self; 12] = [
        Self::Building,
        Self::Techno,
        Self::Country,
        Self::Sound,
        Self::Eva,
        Self::Theme,
        Self::Movie,
        Self::Animation,
        Self::Weapon,
        Self::SuperWeapon,
        Self::Warhead,
        Self::ParticleSystem,
    ];

    pub const fn collection(self) -> &'static str {
        match self {
            Self::Building => "buildings",
            Self::Techno => "technos",
            Self::Country => "countries",
            Self::Sound => "sounds",
            Self::Eva => "eva",
            Self::Theme => "themes",
            Self::Movie => "movies",
            Self::Animation => "animations",
            Self::Weapon => "weapons",
            Self::SuperWeapon => "superWeapons",
            Self::Warhead => "warheads",
            Self::ParticleSystem => "particleSystems",
        }
    }
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Techno => "techno",
            Self::Country => "country",
            Self::Sound => "sound",
            Self::Eva => "eva",
            Self::Theme => "theme",
            Self::Movie => "movie",
            Self::Animation => "animation",
            Self::Weapon => "weapon",
            Self::SuperWeapon => "superWeapon",
            Self::Warhead => "warhead",
            Self::ParticleSystem => "particleSystem",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Catalog {
    /// Host-owned rule snapshots; never sent to the TypeScript evaluator.
    #[serde(skip)]
    pub vehicle_templates: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(skip)]
    pub profile: String,
    #[serde(skip)]
    pub type_projection: BTreeMap<String, u64>,
    #[serde(rename = "nativeDefinitions")]
    pub native_definitions: BTreeMap<String, Vec<String>>,
    pub houses: Vec<String>,
    pub waypoints: Vec<u32>,
    pub units: Vec<String>,
    pub objects: Vec<String>,
    pub strings: Vec<String>,
    pub buildings: Vec<String>,
    pub technos: Vec<String>,
    pub countries: Vec<String>,
    pub sounds: Vec<String>,
    pub eva: Vec<String>,
    pub themes: Vec<String>,
    pub movies: Vec<String>,
    pub animations: Vec<String>,
    pub weapons: Vec<String>,
    #[serde(rename = "superWeapons")]
    pub super_weapons: Vec<String>,
    pub warheads: Vec<String>,
    #[serde(rename = "particleSystems")]
    pub particle_systems: Vec<String>,
    #[serde(rename = "houseColors")]
    pub house_colors: Vec<String>,
    /// Original registry order, independent of sorted authoring choices.
    /// Absent entries deliberately cannot be encoded as numeric operands.
    #[serde(skip)]
    pub registry_order: BTreeMap<String, Vec<String>>,
    /// Authoritative fixed AIMD scripts; None refuses Script17 numeric encoding.
    #[serde(skip)]
    pub fixed_scripts: Option<Vec<String>>,
    /// Fixed TeamType Script references visited before any explicit ScriptTypes.
    #[serde(skip)]
    pub fixed_team_scripts: Vec<String>,
    /// Country subset whose indices are fixed before any map/mode extension.
    #[serde(skip)]
    pub fixed_country_indices: BTreeMap<String, i32>,
    /// Production Country Name/ID resolution, including map aliases.
    #[serde(skip)]
    pub country_names: BTreeMap<String, String>,
    /// Authoring-only descriptions, grouped by MapSchema collection and original ID.
    #[serde(skip)]
    pub descriptions: BTreeMap<String, BTreeMap<String, String>>,
}
impl Catalog {
    pub fn from_document(doc: &MapDocument, units: Vec<String>, strings: Vec<String>) -> Self {
        let records = doc.records();
        let mut result = Self {
            houses: mission::ids(doc, Kind::House),
            waypoints: records
                .values()
                .filter(|r| r.section == "Waypoints")
                .filter_map(|r| r.key.parse().ok())
                .collect(),
            objects: records
                .values()
                .filter(|r| startup_owner::tag_column(&r.section).is_some())
                .map(|r| format!("{}:{}", r.section, r.key))
                .collect(),
            units,
            strings,
            descriptions: BTreeMap::new(),
            ..Default::default()
        };
        result.houses.extend(["Neutral".into(), "Special".into()]);
        result.houses.sort();
        result.houses.dedup();
        result.waypoints.sort();
        result.waypoints.dedup();
        result.units.sort();
        result.units.dedup();
        result.objects.sort();
        result.objects.dedup();
        result.strings.sort();
        result.strings.dedup();
        result
    }

    pub fn resources(&self, kind: ResourceKind) -> &[String] {
        match kind {
            ResourceKind::Building => &self.buildings,
            ResourceKind::Techno => &self.technos,
            ResourceKind::Country => &self.countries,
            ResourceKind::Sound => &self.sounds,
            ResourceKind::Eva => &self.eva,
            ResourceKind::Theme => &self.themes,
            ResourceKind::Movie => &self.movies,
            ResourceKind::Animation => &self.animations,
            ResourceKind::Weapon => &self.weapons,
            ResourceKind::SuperWeapon => &self.super_weapons,
            ResourceKind::Warhead => &self.warheads,
            ResourceKind::ParticleSystem => &self.particle_systems,
        }
    }

    /// Preserve each registered slot, including repeated names: later positions
    /// must not shift when a malformed or duplicate slot precedes them.
    pub fn register(&mut self, kind: ResourceKind, ordered: Vec<String>) {
        let mut choices = ordered.clone();
        choices.sort();
        choices.dedup();
        let target = match kind {
            ResourceKind::Building => &mut self.buildings,
            ResourceKind::Techno => &mut self.technos,
            ResourceKind::Country => &mut self.countries,
            ResourceKind::Sound => &mut self.sounds,
            ResourceKind::Eva => &mut self.eva,
            ResourceKind::Theme => &mut self.themes,
            ResourceKind::Movie => &mut self.movies,
            ResourceKind::Animation => &mut self.animations,
            ResourceKind::Weapon => &mut self.weapons,
            ResourceKind::SuperWeapon => &mut self.super_weapons,
            ResourceKind::Warhead => &mut self.warheads,
            ResourceKind::ParticleSystem => &mut self.particle_systems,
        };
        *target = choices;
        self.registry_order
            .insert(kind.collection().into(), ordered);
    }

    pub fn resource_id<'a>(
        &'a self,
        reference: &crate::native::Reference,
        expected: ResourceKind,
    ) -> Result<&'a str> {
        let id = reference.check(expected.kind())?;
        ensure!(
            !id.is_empty()
                && id.is_ascii()
                && !id.contains([',', ';', '\r', '\n', '\0'])
                && id.trim() == id,
            "{} 引用 {id:?} 不能编码为原生资源名称",
            expected.kind()
        );
        self.resources(expected)
            .iter()
            .find(|value| value.as_str() == id)
            .map(String::as_str)
            .with_context(|| format!("{} 引用 {id} 不在当前资源目录中", expected.kind()))
    }

    pub fn resource_index(
        &self,
        reference: &crate::native::Reference,
        expected: ResourceKind,
    ) -> Result<i32> {
        let id = self.resource_id(reference, expected)?;
        if expected == ResourceKind::Country
            && let Some(index) = self.fixed_country_indices.get(id)
        {
            return Ok(*index);
        }
        let order = self
            .registry_order
            .get(expected.collection())
            .with_context(|| {
                format!("{} 缺少原生注册顺序，不能使用显示目录序号", expected.kind())
            })?;
        let positions: Vec<_> = order
            .iter()
            .enumerate()
            .filter(|(_, value)| value.eq_ignore_ascii_case(id))
            .map(|(index, _)| index)
            .collect();
        ensure!(
            positions.len() == 1,
            "{} 引用 {id} 的注册序号缺失或有歧义",
            expected.kind()
        );
        i32::try_from(positions[0]).context("资源注册序号超出原生范围")
    }
    pub fn typescript(&self) -> Result<String> {
        let mut source = "// Generated from the current map and resource profile. Refresh in the editor.\n// 当前地图的引用枚举；请通过编辑器刷新，不要手工修改。\n\n".to_owned();
        let schema = serde_json::to_value(self)?;
        let mut collections = Vec::new();
        for (collection, name, label) in [
            ("houses", "House", "所属方"),
            ("waypoints", "Waypoint", "航点"),
            ("units", "Unit", "单位类型"),
            ("objects", "MapObject", "地图对象"),
            ("strings", "Text", "文本标签"),
            ("buildings", "Building", "建筑类型"),
            ("particleSystems", "ParticleSystem", "粒子系统类型"),
            ("technos", "Techno", "科技类型（含建筑）"),
            ("countries", "Country", "国家"),
            ("sounds", "Sound", "声音"),
            ("eva", "Eva", "EVA 语音"),
            ("themes", "Theme", "音乐"),
            ("movies", "Movie", "影片"),
            ("animations", "Animation", "动画"),
            ("weapons", "Weapon", "武器类型"),
            ("superWeapons", "SuperWeapon", "超级武器"),
            ("warheads", "Warhead", "弹头"),
            ("houseColors", "HouseColor", "House 颜色方案"),
        ] {
            source.push_str(&format!(
                "/** 当前地图可用的{label}。成员值保留原始 ID 或编号。 */\nexport enum {name} {{\n"
            ));
            let values = schema[collection].as_array().expect("catalog collection");
            let members = enum_members(values, collection == "waypoints");
            for (id, member) in values.iter().zip(members) {
                let literal = serde_json::to_string(id)?;
                let key = id.as_str().map_or_else(|| id.to_string(), str::to_owned);
                let description = self
                    .descriptions
                    .get(collection)
                    .and_then(|items| items.get(&key));
                let description = description
                    .cloned()
                    .unwrap_or_else(|| format!("{label}：{key}"));
                // Resource text must not terminate a comment or inject JSDoc tags/source lines.
                let description = description
                    .replace("*/", "* /")
                    .replace(['\r', '\n', '\u{2028}', '\u{2029}'], " ");
                source.push_str(&format!(
                    "  /** {description} */\n  {member} = {literal},\n"
                ));
            }
            source.push_str("}\n\n");
            // Enum types already enumerate exactly the available IDs. Avoid
            // inferring a thousands-element tuple for large CSF/object lists.
            let array = if values.is_empty() {
                "[] as readonly never[]".to_owned()
            } else {
                // Each member already appears in the enum. Referencing every
                // member again in a contextual array forces repeated union
                // assignability checks for large CSF catalogs.
                let filter = if collection == "waypoints" {
                    ".filter(key => isNaN(Number(key)))"
                } else {
                    ""
                };
                format!(
                    "Object.keys({name}){filter}.map(key => {name}[key as keyof typeof {name}]) as readonly {name}[]"
                )
            };
            collections.push(format!("  {collection}: {array}"));
        }
        source.push_str(
            "/** 当前地图的类型安全引用目录，由上述枚举组成。 */\nexport const map = {\n",
        );
        source.push_str(&collections.join(",\n"));
        source.push_str("\n} as const;\n");
        Ok(source)
    }

    /// Resolve the generated enum member without duplicating collision handling in templates.
    pub fn enum_member(&self, collection: &str, id: &str) -> Result<String> {
        let schema = serde_json::to_value(self)?;
        let values = schema[collection].as_array().context("未知的引用目录")?;
        values
            .iter()
            .zip(enum_members(values, collection == "waypoints"))
            .find(|(value, _)| {
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned)
                    == id
            })
            .map(|(_, member)| member)
            .context("引用不在当前目录中")
    }
}

fn enum_members(values: &[serde_json::Value], numeric: bool) -> Vec<String> {
    let mut used = BTreeSet::new();
    values
        .iter()
        .map(|value| {
            let id = value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned);
            let mut base: String = id
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect();
            if numeric {
                base.insert_str(0, "WP");
            } else if base.is_empty()
                || base.starts_with(|ch: char| ch.is_ascii_digit())
                || base == "__proto__"
            {
                base.insert(0, '_');
            }
            let mut member = base.clone();
            let mut suffix = 2;
            while !used.insert(member.clone()) {
                member = format!("{base}_{suffix}");
                suffix += 1;
            }
            member
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::Reference;

    #[test]
    fn sorted_choices_never_replace_native_registration_positions() {
        let mut catalog = Catalog::default();
        catalog.register(
            ResourceKind::Sound,
            vec!["Z_FIRST".into(), "A_SECOND".into()],
        );
        assert_eq!(catalog.sounds, ["A_SECOND", "Z_FIRST"]);
        let reference = Reference {
            kind: "sound".into(),
            id: "Z_FIRST".into(),
        };
        assert_eq!(
            catalog
                .resource_index(&reference, ResourceKind::Sound)
                .unwrap(),
            0
        );
        assert!(
            catalog
                .resource_index(&reference, ResourceKind::Eva)
                .is_err()
        );
        catalog.registry_order.clear();
        assert!(
            catalog
                .resource_index(&reference, ResourceKind::Sound)
                .is_err()
        );
        catalog.register(
            ResourceKind::Sound,
            vec!["DUP".into(), "dup".into(), "Z_FIRST".into()],
        );
        assert_eq!(
            catalog
                .resource_index(&reference, ResourceKind::Sound)
                .unwrap(),
            2
        );
        let duplicate = Reference {
            kind: "sound".into(),
            id: "DUP".into(),
        };
        assert!(
            catalog
                .resource_index(&duplicate, ResourceKind::Sound)
                .is_err()
        );
        let missing = Reference {
            kind: "sound".into(),
            id: "missing".into(),
        };
        assert!(catalog.resource_id(&missing, ResourceKind::Sound).is_err());
    }
}
