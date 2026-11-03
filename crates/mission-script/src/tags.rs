//! Shared Tag definitions and transactional authored bindings.
use super::*;

pub(super) struct Tags {
    pub ids: BTreeMap<String, String>,
    pub roots: BTreeMap<String, String>,
}
impl Tags {
    pub fn new(
        plan: &Mission,
        graph: &rules::Graph<'_>,
        put: &mut impl FnMut(&str, &str, String) -> Result<()>,
    ) -> Result<Self> {
        let mut ids = BTreeMap::new();
        let mut roots = BTreeMap::new();
        for tag in &plan.tags {
            identifier(&tag.id)?;
            let root = tag.rule.check("rule")?;
            named(&graph.ids, &tag.rule, "rule")?;
            ensure!(
                graph.roots.get(root).is_some_and(|r| *r == root),
                "Tag {} 必须引用关联链首规则 {root}",
                tag.id
            );
            let id = physical(&plan.id, &tag.id, "ET");
            ensure!(
                ids.insert(tag.id.clone(), id.clone()).is_none(),
                "重复 Tag {}",
                tag.id
            );
            ensure!(
                roots.insert(root.into(), id.clone()).is_none(),
                "规则链 {root} 已有显式 Tag，不能重复创建活动实例"
            );
            let rule = plan.rules.iter().find(|rule| rule.id == root).unwrap();
            let repeat = tag.repeat.unwrap_or(if rule.once { 0 } else { 2 });
            ensure!(
                repeat <= 2 && (repeat == 2) != rule.once,
                "Tag {} 的重复方式与规则 once 不一致",
                tag.id
            );
            put(
                "Tags",
                &id,
                format!("{repeat},{},{}", tag.id, graph.ids[root]),
            )?;
        }
        for rule in &plan.rules {
            if graph.roots[rule.id.as_str()] == rule.id && !roots.contains_key(&rule.id) {
                let id = physical(&plan.id, &rule.id, "TG");
                let repeat = if rule.once { 0 } else { 2 };
                put(
                    "Tags",
                    &id,
                    format!("{repeat},{},{}", rule.id, graph.ids[&rule.id]),
                )?;
                roots.insert(rule.id.clone(), id);
            }
        }
        Ok(Self { ids, roots })
    }
    pub fn bind(
        &self,
        plan: &Mission,
        events: &mut rules::Events<'_>,
        put: &mut impl FnMut(&str, &str, String) -> Result<()>,
    ) -> Result<()> {
        let mut cells = BTreeSet::new();
        let mut objects = BTreeSet::new();
        for binding in &plan.bindings {
            match binding {
                Binding::Object { object, tag } => {
                    ensure!(
                        objects.insert(&object.id),
                        "对象 {} 重复显式绑定 Tag",
                        object.id
                    );
                    let tag = named(&self.ids, tag, "tag")?;
                    events.bind_object(object, &tag)?;
                }
                Binding::Cell { x, y, tag } => {
                    let key = mission::cell_key(events.doc, [*x, *y])?;
                    ensure!(cells.insert(key.clone()), "格子 ({x},{y}) 重复绑定 Tag");
                    ensure!(
                        events.doc.ini_value("CellTags", &key).is_none(),
                        "格子 ({x},{y}) 已有手工 CellTag，不能覆盖"
                    );
                    put("CellTags", &key, named(&self.ids, tag, "tag")?)?;
                }
            }
        }
        Ok(())
    }
}
