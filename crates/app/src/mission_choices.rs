//! Shared form candidates; pending edits, history, language and resources invalidate them.
use crate::{
    i18n::{self, tr},
    map::MapScene,
};
use eframe::egui;
use ra2_map_assets::catalog::Catalog;
use ra2_map_document::{
    document::MapDocument,
    mission::{self, Kind, schema::ParameterKind},
};
use std::{collections::BTreeMap, sync::Arc};

pub(super) type Choices = Arc<Vec<(String, String)>>;
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    Reference(Kind),
    Waypoint(bool),
    Type(&'static str),
    Country,
}
#[derive(Clone, Default)]
struct Cache {
    catalog: Option<Arc<Catalog>>,
    definitions: Option<Arc<mission::ExternalDefinitions>>,
    language: Option<i18n::Language>,
    document: Option<(u64, u64)>,
    static_choices: BTreeMap<Family, Choices>,
    document_choices: BTreeMap<Family, Choices>,
}
impl Cache {
    fn get(
        &mut self,
        family: Family,
        doc: &MapDocument,
        catalog: &Arc<Catalog>,
        definitions: &Arc<mission::ExternalDefinitions>,
    ) -> Choices {
        let language = i18n::selected_language();
        if self.language != Some(language)
            || self
                .catalog
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, catalog))
        {
            self.static_choices.clear();
            self.document_choices.clear();
            self.language = Some(language);
            self.catalog = Some(catalog.clone());
        }
        let identity = (doc.id, doc.edit_revision());
        if self.document != Some(identity)
            || self
                .definitions
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, definitions))
        {
            self.document_choices.clear();
            self.document = Some(identity);
            self.definitions = Some(definitions.clone());
        }
        let choices = if matches!(family, Family::Type(_) | Family::Country) {
            &mut self.static_choices
        } else {
            &mut self.document_choices
        };
        choices
            .entry(family)
            .or_insert_with(|| {
                Arc::new(match family {
                    Family::Reference(kind) => definitions
                        .candidates(doc, kind)
                        .into_iter()
                        .map(|(id, fixed)| {
                            let label = if kind == Kind::Global {
                                format!("{id} · {}", doc.ini_text("EditorGlobalNames", &id))
                            } else {
                                format!(
                                    "{id} · {}",
                                    if fixed {
                                        definitions.source.as_str()
                                    } else {
                                        tr!("地图")
                                    }
                                )
                            };
                            (id, label)
                        })
                        .collect(),
                    Family::Waypoint(letters) => doc
                        .record_iter()
                        .filter(|(_, record)| record.section == "Waypoints")
                        .filter_map(|(_, record)| {
                            let index = record.key.parse::<u32>().ok()?;
                            let token = if letters {
                                mission::schema::waypoint_token(index).ok()?
                            } else {
                                index.to_string()
                            };
                            Some((token.clone(), format!("{} · {token}", record.type_id())))
                        })
                        .collect(),
                    Family::Type(section) => catalog
                        .object_types
                        .iter()
                        .filter(|(kind, _)| {
                            (section.is_empty() || section == "Foot")
                                && matches!(
                                    kind.as_str(),
                                    "Infantry" | "Units" | "Aircraft" | "Structures"
                                )
                                && (section != "Foot" || kind.as_str() != "Structures")
                                || kind.as_str() == section
                        })
                        .flat_map(|(_, ids)| ids)
                        .map(|id| (id.clone(), catalog.object_label(id).into_owned()))
                        .collect(),
                    Family::Country => {
                        let mut choices: Vec<_> = catalog
                            .rules
                            .section_or_empty("Countries")
                            .registry_ids()
                            .into_iter()
                            .enumerate()
                            .map(|(index, id)| (index.to_string(), format!("{index} · {id}")))
                            .collect();
                        choices.extend([
                            ("-1".into(), tr!("-1 · 事件任意国家 / 动作无目标").into()),
                            ("8997".into(), tr!("8997 · 动作保留所属方").into()),
                        ]);
                        choices
                    }
                })
            })
            .clone()
    }
}

pub(super) fn get(
    ctx: &egui::Context,
    kind: ParameterKind,
    doc: &MapDocument,
    scene: &MapScene,
) -> Choices {
    let family = match kind {
        ParameterKind::Reference(kind) => Family::Reference(kind),
        ParameterKind::Waypoint { letters } => Family::Waypoint(letters),
        ParameterKind::Type(section) => Family::Type(section),
        ParameterKind::Country => Family::Country,
        _ => return Arc::default(),
    };
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Cache>(egui::Id::unique("mission-candidates"))
            .get(family, doc, &scene.catalog, &scene.mission_definitions)
    })
}

pub(super) fn references(
    ctx: &egui::Context,
    kind: Kind,
    doc: &MapDocument,
    definitions: &Arc<mission::ExternalDefinitions>,
) -> Choices {
    ctx.data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<Cache>(egui::Id::unique("mission-candidates"));
        let catalog = cache.catalog.clone().unwrap_or_default();
        cache.get(Family::Reference(kind), doc, &catalog, definitions)
    })
}

#[cfg(test)]
#[path = "mission_choices_tests.rs"]
mod tests;
