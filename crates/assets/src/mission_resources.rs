//! Cached named choices; loaded on the installation worker, never in UI frames.
use std::collections::BTreeMap;
use vera20k_core::{
    assets::{asset_manager::AssetManager, csf_file::CsfFile},
    rules::{
        ini_parser::IniFile,
        sound_ini::{EvaRegistry, SoundRegistry},
    },
};
#[derive(Default)]
pub struct MissionResources {
    pub choices: BTreeMap<String, Vec<(String, String)>>,
    pub errors: Vec<String>,
}
pub(crate) fn load(manager: &AssetManager) -> MissionResources {
    let mut out = MissionResources::default();
    if let Some(bytes) = manager.get_ref(manager.ini_roots().art())
        && let Ok(art) = IniFile::from_bytes(bytes)
    {
        out.choices.insert(
            "Movie".into(),
            movie_names(&art)
                .into_iter()
                .map(|id| (id.clone(), id))
                .collect(),
        );
    }
    for (name, file) in [
        ("Sound", manager.ini_roots().sound()),
        ("Eva", manager.ini_roots().eva()),
    ] {
        match manager
            .get_ref(file)
            .ok_or_else(|| format!("缺少 {file}"))
            .and_then(|bytes| IniFile::from_bytes(bytes).map_err(|e| e.to_string()))
        {
            Ok(ini) => {
                let ids = audio_names(name, &ini);
                out.choices.insert(
                    name.into(),
                    ids.into_iter().map(|id| (id.clone(), id)).collect(),
                );
            }
            Err(e) => out.errors.push(e),
        }
    }
    out.choices.insert(
        "Theme".into(),
        vera20k_core::rules::theme_ini::ThemeCatalog::select(manager)
            .entries()
            .iter()
            .map(|t| (t.key.clone(), t.key.clone()))
            .collect(),
    );
    match CsfFile::load_active(|name| manager.get_ref(name)) {
        Ok((table, _)) => {
            let mut entries: Vec<_> = table
                .entries()
                .map(|(label, text)| (label.to_owned(), format!("{label} · {text}")))
                .collect();
            entries.sort();
            out.choices.insert("Csf".into(), entries);
        }
        Err(e) => out.errors.push(e.to_string()),
    }
    out
}

/// Immutable presentation projection of one concrete InfantryType.
#[derive(Clone)]
pub struct InfantryAppearance {
    pub image: String,
    pub shp_loads: Vec<vera20k_core::rules::object_image::ObjectImageLoad>,
    pub sequences: vera20k_core::rules::animation_sequence::SequenceSet,
}

/// Registries and appearance data from the same cold/reset Rules owner as game
/// loading, including LANG, map passes and implicit identities.
pub struct NativeRegistries {
    pub radar: std::sync::Arc<crate::radar::Palette>,
    pub infantry_appearances: BTreeMap<String, InfantryAppearance>,
    pub vehicle_templates: BTreeMap<String, BTreeMap<String, String>>,
    pub countries: BTreeMap<String, i32>,
    /// Exact production alias-before-ID lookup, folded only for matching.
    pub country_names: BTreeMap<String, String>,
    pub type_orders: BTreeMap<String, Vec<String>>,
}

#[cfg(test)]
pub(crate) fn native_registries(manager: &AssetManager) -> Result<NativeRegistries, String> {
    scenario_registries(manager, &IniFile::empty(), false, None)
}

/// Use the production cold Rules owner, including map/mode implicit allocations.
pub fn scenario_registries(
    manager: &AssetManager,
    map: &IniFile,
    multiplayer: bool,
    mode: Option<&IniFile>,
) -> Result<NativeRegistries, String> {
    use std::sync::Arc;
    use vera20k_core::rules::{
        process_owner::NativeScenarioRulesPrefix,
        retail_sources::{RetailRulesSources, select_ini},
    };
    let audio = |name: &str| -> Result<IniFile, String> {
        if manager.resolve_ref(name).is_none() {
            return Ok(IniFile::empty());
        }
        select_ini(manager, name).map(|selected| selected.ini)
    };
    let sounds = SoundRegistry::from_ini(&audio(manager.ini_roots().sound())?);
    let eva = EvaRegistry::from_ini(&audio(manager.ini_roots().eva())?);
    let mut owner =
        RetailRulesSources::select(manager)?.into_owner(Arc::new(sounds), Arc::new(eva))?;
    let (mut rules, processed_ini, _, _) = owner
        .load_scenario(
            if multiplayer {
                NativeScenarioRulesPrefix::NonCampaign(mode)
            } else {
                NativeScenarioRulesPrefix::Campaign(mode)
            },
            map,
        )
        .map_err(|error| error.to_string())?
        .into_parts();
    let mut country_names = BTreeMap::new();
    for (id, alias) in rules.cooperative_country_candidates() {
        for token in alias.into_iter().chain(std::iter::once(id)) {
            country_names
                .entry(token.to_ascii_lowercase())
                .or_insert_with(|| id.to_string());
        }
    }
    let countries = rules
        .cooperative_country_candidates()
        .filter_map(|(id, _)| {
            rules
                .country_index(id)
                .map(|index| (id.to_owned(), i32::from(index.0)))
        })
        .collect();
    let animations = (0..)
        .map_while(|index| rules.anim_type_name_by_index(index).map(str::to_owned))
        .collect();
    // None also denotes an ambiguous slot, so do not stop at the first hole.
    // Empty placeholders retain native ordinals without offering unsafe IDs.
    let weapons = (0..rules.weapon_array_len())
        .map(|index| {
            i32::try_from(index)
                .ok()
                .and_then(|index| rules.weapon_by_array_index(index))
                .map_or_else(String::new, |weapon| weapon.id.clone())
        })
        .collect();
    let particles = (0..)
        .map_while(|index| rules.particle_system_type_id_by_index(index))
        .map(|id| rules.particle_system_type(id).name.clone())
        .collect();
    let theater = map
        .section_or_empty("Map")
        .read_name("Theater", 32)
        .unwrap_or("TEMPERATE");
    let extension = vera20k_core::map::theater::theater_extension(theater).unwrap_or("tem");
    rules.bind_terrain_assets(
        manager,
        extension,
        vera20k_core::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
    );
    let radar = Arc::new(crate::radar::Palette::load(
        &rules, manager, theater, extension,
    ));
    Ok(NativeRegistries {
        radar,
        infantry_appearances: rules
            .all_objects()
            .filter(|object| {
                object.category == vera20k_core::rules::object_type::ObjectCategory::Infantry
            })
            .filter_map(|object| {
                rules
                    .animation_sequence_in_category(object.category, &object.id)
                    .cloned()
                    .map(|sequences| {
                        (
                            object.id.to_ascii_uppercase(),
                            InfantryAppearance {
                                image: object.image.clone(),
                                shp_loads: object.shp_image_loads().to_vec(),
                                sequences,
                            },
                        )
                    })
            })
            .collect(),
        vehicle_templates: rules
            .vehicle_ids
            .iter()
            .filter_map(|id| {
                let section = processed_ini.section(id)?;
                let mut fields: BTreeMap<_, _> = section
                    .raw_entries()
                    .map(|(key, value)| (key.to_owned(), value.to_owned()))
                    .collect();
                if let Some(object) = rules.object(id) {
                    fields
                        .entry("Image".into())
                        .or_insert_with(|| object.image.clone());
                }
                Some((id.clone(), fields))
            })
            .collect(),
        countries,
        country_names,
        type_orders: BTreeMap::from([
            ("buildings".into(), rules.building_ids.clone()),
            ("infantry".into(), rules.infantry_ids.clone()),
            ("vehicles".into(), rules.vehicle_ids.clone()),
            ("aircraft".into(), rules.aircraft_ids.clone()),
            ("superWeapons".into(), rules.super_weapon_order.clone()),
            ("animations".into(), animations),
            (
                "voxelAnims".into(),
                rules
                    .voxel_anim_types_iter()
                    .map(|ty| ty.name.clone())
                    .collect(),
            ),
            ("weapons".into(), weapons),
            ("particleSystems".into(), particles),
        ]),
    })
}

pub fn audio_names(kind: &str, ini: &IniFile) -> Vec<String> {
    if kind == "Sound" {
        let registry = SoundRegistry::from_ini(ini);
        (0..registry.len())
            .filter_map(|index| registry.entry_by_index(index as i32))
            .map(|entry| entry.id.clone())
            .collect()
    } else {
        let registry = EvaRegistry::from_ini(ini);
        (0..registry.len())
            .filter_map(|index| registry.entry_by_index(index as i32))
            .map(|entry| entry.name.clone())
            .collect()
    }
}

/// Adapter for MovieRegistry's private ART reader: ReadString32, source order,
/// first case-insensitive name. Bare `none` is a name; `<none>` is a sentinel.
fn movie_names(art: &IniFile) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    if let Some(movies) = art.section("Movies") {
        for key in movies.keys() {
            let name = movies.read_string(key, "<none>", 32);
            if !name.is_empty()
                && (name.eq_ignore_ascii_case("<none>")
                    || !names.iter().any(|old| old.eq_ignore_ascii_case(&name)))
            {
                names.push(name);
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_country_indices_follow_native_sources_and_do_not_use_display_order() {
        let fixture = crate::test_support::Fixture::new();
        std::fs::write(fixture.0.join("rulesmd.ini"), b"[Countries]\n42=ZCountry\n0=ACountry\n[ZCountry]\nName=First\n[ACountry]\nName=Second\n").unwrap();
        std::fs::write(fixture.0.join("artmd.ini"), b"[Movies]\n").unwrap();
        let install = fixture.install();
        let registries = native_registries(&install.manager).unwrap();
        assert_eq!(registries.country_names["first"], "ZCountry");
        assert_eq!(registries.country_names["zcountry"], "ZCountry");
        assert_eq!(registries.country_names["second"], "ACountry");
        let indices = registries.countries;
        assert_eq!(indices["ZCountry"], 0);
        assert_eq!(indices["ACountry"], 1);
        use std::sync::Arc;
        use vera20k_core::rules::{
            process_owner::NativeScenarioRulesPrefix, retail_sources::RetailRulesSources,
        };
        let mut owner = RetailRulesSources::select(&install.manager)
            .unwrap()
            .into_owner(
                Arc::new(SoundRegistry::default()),
                Arc::new(EvaRegistry::default()),
            )
            .unwrap();
        let mode = IniFile::from_str("[Countries]\n0=ModeCountry\n[ModeCountry]\nName=Mode\n");
        let map = IniFile::from_str("[Countries]\n0=MapCountry\n[MapCountry]\nName=Map\n");
        let (rules, _, _, _) = owner
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(Some(&mode)), &map)
            .unwrap()
            .into_parts();
        for (id, index) in indices {
            assert_eq!(i32::from(rules.country_index(&id).unwrap().0), index);
        }
        assert!(rules.country_index("ModeCountry").is_some());
        assert!(rules.country_index("MapCountry").is_some());
        // Native lookup visits each Country's alias before its ID, retaining
        // the first match even when a later Country has the matching ID.
        let collisions = IniFile::from_str("[Countries]\n0=First\n[First]\nName=ZCountry\n");
        let projected = scenario_registries(&install.manager, &collisions, false, None).unwrap();
        assert_eq!(projected.country_names["first"], "ZCountry");
        assert_eq!(projected.country_names["zcountry"], "ZCountry");
    }

    #[test]
    fn scenario_weapons_keep_implicit_allocations_and_ambiguous_slot_positions() {
        let fixture = crate::test_support::Fixture::new();
        std::fs::write(fixture.0.join("rulesmd.ini"), b"[VehicleTypes]\n0=CarrierA\n1=CarrierB\n2=CarrierC\n[CarrierA]\nPrimary=ABCDEFGHIJKLMNOPQRSTUVWXA\n[CarrierB]\nPrimary=ABCDEFGHIJKLMNOPQRSTUVWXB\n[CarrierC]\nPrimary=BaseWeapon\n").unwrap();
        std::fs::write(fixture.0.join("artmd.ini"), b"[Movies]\n").unwrap();
        let install = fixture.install();
        let mode =
            IniFile::from_str("[VehicleTypes]\n3=ModeCarrier\n[ModeCarrier]\nPrimary=ModeWeapon\n");
        let map =
            IniFile::from_str("[VehicleTypes]\n4=MapCarrier\n[MapCarrier]\nPrimary=MapWeapon\n");
        let projected = scenario_registries(&install.manager, &map, true, Some(&mode)).unwrap();
        assert_eq!(
            projected.type_orders["weapons"],
            ["", "", "BaseWeapon", "ModeWeapon", "MapWeapon"]
        );
        let base = scenario_registries(&install.manager, &IniFile::empty(), true, None).unwrap();
        assert_eq!(base.type_orders["weapons"], ["", "", "BaseWeapon"]);
    }

    #[test]
    fn mission_media_choices_use_registered_slots_instead_of_raw_list_rows() {
        let sounds = IniFile::from_str("[SoundList]\n42=ZFirst\n0=ASecond\n99=zfirst\n3=Third\n");
        assert_eq!(
            audio_names("Sound", &sounds),
            ["ZFirst", "ASecond", "Third"]
        );
        let eva = IniFile::from_str("[DialogList]\n42=ZFirst\n0=ASecond\n99=zfirst\n3=Third\n");
        assert_eq!(audio_names("Eva", &eva), ["ZFirst", "ASecond", "Third"]);
        let art = IniFile::from_str("[Movies]\n42=ZFirst\n0=ASecond\n99=zfirst\n3=none\n");
        assert_eq!(movie_names(&art), ["ZFirst", "ASecond", "none"]);
        let long = "A".repeat(40);
        let art = IniFile::from_str(&format!(
            "[Movies]\n0={long}\n1={}\n2=Next\n",
            "A".repeat(31)
        ));
        assert_eq!(movie_names(&art), ["A".repeat(31), "Next".into()]);
    }
}
