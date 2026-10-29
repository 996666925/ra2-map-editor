//! Installation-owned mission references. Kept out of the editable document.
use ra2_map_assets::assets::Install;
use ra2_map_document::mission::ExternalDefinitions;

pub(crate) fn load(install: &Install) -> ExternalDefinitions {
    let source = install.manager.ini_roots().ai().to_owned();
    match install.fixed_team_definitions() {
        Ok(registry) => ExternalDefinitions {
            source,
            teams: registry.team_types.iter().map(|r| r.id.clone()).collect(),
            scripts: registry.scripts.iter().map(|r| r.id.clone()).collect(),
            task_forces: registry.task_forces.iter().map(|r| r.id.clone()).collect(),
            ai_triggers: registry.ai_triggers.iter().map(|r| r.id.clone()).collect(),
            ai_trigger_rows: registry
                .ai_triggers
                .iter()
                .map(|r| (r.id.clone(), r.tokens.join(",")))
                .collect(),
            buildings: install
                .catalog
                .object_types
                .get("Structures")
                .into_iter()
                .flatten()
                .cloned()
                .collect(),
            object_types: install
                .catalog
                .object_types
                .iter()
                .filter(|(section, _)| {
                    matches!(
                        section.as_str(),
                        "Structures" | "Units" | "Infantry" | "Aircraft"
                    )
                })
                .flat_map(|(_, ids)| ids.iter().cloned())
                .collect(),
            countries: install
                .catalog
                .rules
                .section("Countries")
                .map(|s| s.registry_ids().into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            error: None,
        },
        Err(error) => ExternalDefinitions {
            source,
            error: Some(error),
            ..Default::default()
        },
    }
}
