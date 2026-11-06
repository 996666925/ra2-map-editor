//! Conservative dependencies of the static scene on authored INI fields.
use ra2_map_assets::catalog::Catalog;
use std::collections::{HashMap, HashSet};
use vera20k_core::rules::ini_parser::IniFile;

pub(crate) struct MissionFields {
    visual_sections: HashSet<String>,
    definitions: HashMap<String, Option<&'static str>>,
}

impl MissionFields {
    pub fn new(
        before: &IniFile,
        after: &IniFile,
        catalog: &Catalog,
        installation: &Catalog,
    ) -> Self {
        // A mission ID can also name a Mod type, weapon, country or placed owner.
        // Installation-owned sections and native type ordinals are never skipped.
        let mut visual_sections: HashSet<_> = installation
            .rules
            .section_names()
            .iter()
            .map(|section| section.to_ascii_lowercase())
            .collect();
        visual_sections.extend(
            catalog
                .object_types
                .values()
                .flatten()
                .chain(catalog.mission_registries.values().flatten())
                .chain(catalog.mission_type_orders.values().flatten())
                .chain(catalog.mission_country_indices.keys())
                .chain(catalog.mission_country_names.values())
                .chain(catalog.overlays.iter())
                .map(|id| id.to_ascii_lowercase()),
        );
        let mut definitions = HashMap::new();
        for ini in [before, after] {
            visual_sections.extend(
                ini.section_or_empty("Houses")
                    .registry_ids()
                    .iter()
                    .map(|id| id.to_ascii_lowercase()),
            );
            for objects in ["Structures", "Units", "Infantry", "Aircraft"] {
                visual_sections.extend(
                    ini.section_or_empty(objects)
                        .raw_entries()
                        .filter_map(|(_, row)| row.split(',').next())
                        .map(|owner| owner.trim().to_ascii_lowercase()),
                );
            }
            for registry in ["ScriptTypes", "TaskForces", "TeamTypes"] {
                for id in ini.section_or_empty(registry).registry_ids() {
                    let kind = definitions
                        .entry(id.to_ascii_lowercase())
                        .or_insert(Some(registry));
                    if *kind != Some(registry) {
                        *kind = None;
                    }
                }
            }
        }
        Self {
            visual_sections,
            definitions,
        }
    }
    pub fn contains(&self, section: &str, key: &str) -> bool {
        let section_lower = section.to_ascii_lowercase();
        if self.visual_sections.contains(&section_lower) {
            return false;
        }
        let key_lower = key.to_ascii_lowercase();
        match section_lower.as_str() {
            "triggers"
            | "events"
            | "actions"
            | "tags"
            | "celltags"
            | "variablenames"
            | "editorglobalnames"
            | "aitriggertypes"
            | "aitriggertypesenable"
            | "scripttypes"
            | "taskforces"
            | "teamtypes" => return true,
            // MultiplayerOnly selects a different native Rules prefix. NewINIFormat,
            // geometry, lighting and all other Basic extensions keep the full path.
            "basic" => {
                return matches!(
                    key_lower.as_str(),
                    "name"
                        | "author"
                        | "homecell"
                        | "althomecell"
                        | "intro"
                        | "brief"
                        | "action"
                        | "theme"
                        | "win"
                        | "lose"
                        | "nextscenario"
                        | "altnextscenario"
                        | "skipmapselect"
                        | "timerinherit"
                        | "carryovermoney"
                        | "carryovercap"
                        | "postscore"
                        | "premapselect"
                        | "endofgame"
                        | "onetimeonly"
                        | "skipscore"
                );
            }
            "map" | "lighting" | "colors" | "countries" | "general" | "ai" | "iq"
            | "audiovisual" | "craterules" | "specialflags" => return false,
            _ => {}
        }
        let numeric = !key.is_empty() && key.bytes().all(|byte| byte.is_ascii_digit());
        match self.definitions.get(&section_lower).copied().flatten() {
            Some("ScriptTypes") => numeric || key_lower == "name",
            Some("TaskForces") => numeric || matches!(key_lower.as_str(), "name" | "group"),
            Some("TeamTypes") => matches!(
                key_lower.as_str(),
                "name"
                    | "house"
                    | "script"
                    | "taskforce"
                    | "waypoint"
                    | "transportwaypoint"
                    | "max"
                    | "priority"
                    | "veteranlevel"
                    | "techlevel"
                    | "group"
                    | "autocreate"
                    | "full"
                    | "recruiter"
                    | "prebuild"
                    | "reinforce"
                    | "suicide"
                    | "aggressive"
                    | "annoyance"
                    | "areteammembersrecruitable"
                    | "avoidthreats"
                    | "droppod"
                    | "guardslower"
                    | "ionimmune"
                    | "isbasedefense"
                    | "istilter"
                    | "looserecruit"
                    | "ontransonly"
                    | "onlytargethouseenemy"
                    | "transportsreturnonunload"
                    | "usetransportorigin"
                    | "whiner"
                    | "mindcontroldecision"
                    | "tag"
            ),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mission_ids_never_override_visual_or_ambiguous_namespaces() {
        let before = IniFile::from_str(
            "[ScriptTypes]\n0=SCRIPT\n1=GI\n2=OWNER\n3=General\n4=DUAL\n5=WEAPON\n[TeamTypes]\n0=DUAL\n[Units]\n0=OWNER,GI,256,20,20,0,Guard,None\n",
        );
        let after = IniFile::empty();
        let mut catalog = Catalog::default();
        catalog
            .object_types
            .insert("Units".into(), vec!["GI".into()]);
        catalog
            .mission_type_orders
            .insert("weapons".into(), vec!["WEAPON".into()]);
        let fields = MissionFields::new(&before, &after, &catalog, &Catalog::default());
        assert!(
            fields.contains("script", "0"),
            "deleted definitions still belong to the mission delta"
        );
        for section in ["gi", "owner", "General", "DUAL", "weapon"] {
            assert!(!fields.contains(section, "Name"), "{section}");
        }
        assert!(!fields.contains("SCRIPT", "Image"));
        let mut installation = Catalog::default();
        installation.rules = IniFile::from_str("[SCRIPT]\nStrength=10\n");
        assert!(
            !MissionFields::new(&before, &after, &catalog, &installation).contains("SCRIPT", "0")
        );
    }
}
