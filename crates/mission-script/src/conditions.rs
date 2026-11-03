//! Lower supported OR expressions to native rules and one owned local latch.
use super::*;
use serde_json::{Value, json};

pub(super) fn lower(plan: &Mission) -> Result<Option<Mission>> {
    if !plan
        .rules
        .iter()
        .any(|rule| matches!(rule.when, Event::Any { .. }))
    {
        return Ok(None);
    }
    let mut value = serde_json::to_value(plan)?;
    let mut helpers = Vec::new();
    let mut branches_by_rule = BTreeMap::<String, Vec<String>>::new();
    let mut used_rules: BTreeSet<String> = plan.rules.iter().map(|rule| rule.id.clone()).collect();
    let mut used_flags: BTreeSet<String> = plan.flags.iter().map(|flag| flag.id.clone()).collect();
    for (index, rule) in plan.rules.iter().enumerate() {
        let Event::Any { events } = &rule.when else {
            continue;
        };
        ensure!(
            rule.linked.is_none()
                && !rule.transfer_on_capture
                && !plan
                    .rules
                    .iter()
                    .any(|other| other.linked.as_ref().is_some_and(|r| r.id == rule.id)),
            "规则 {}：any 不能与关联链/占领转移混用",
            rule.id
        );
        ensure!(
            !plan.tags.iter().any(|tag| tag.rule.id == rule.id),
            "规则 {}：any 辅助分支不能使用显式 Tag",
            rule.id
        );
        fn flatten<'a>(events: &'a [Event], depth: usize, out: &mut Vec<&'a Event>) -> Result<()> {
            ensure!(
                !events.is_empty() && depth <= 16,
                "any 分支不能为空或嵌套超过 16 层"
            );
            for event in events {
                if let Event::Any { events } = event {
                    flatten(events, depth + 1, out)?;
                } else {
                    out.push(event);
                }
                ensure!(out.len() <= 32, "any 最多支持 32 个分支");
            }
            Ok(())
        }
        let mut branches = Vec::new();
        flatten(events, 0, &mut branches)?;
        fn repeatable(event: &Event) -> bool {
            match event {
                Event::All { events } => events.iter().all(repeatable),
                Event::ScenarioTime { .. }
                | Event::EnabledTime { .. }
                | Event::RandomTime { .. }
                | Event::CreditsAtLeast { .. }
                | Event::CreditsAtMost { .. }
                | Event::BuildingsLost { .. }
                | Event::UnitsLost { .. }
                | Event::NoFactories
                | Event::AmbientAtMost { .. }
                | Event::AmbientAtLeast { .. }
                | Event::LowPower { .. }
                | Event::FullPower { .. }
                | Event::CountdownElapsed
                | Event::FlagSet { .. }
                | Event::FlagClear { .. }
                | Event::LocalSet { .. }
                | Event::LocalClear { .. }
                | Event::GlobalSet { .. }
                | Event::GlobalClear { .. }
                | Event::TechnoExists { .. }
                | Event::TechnoAbsent { .. } => true,
                _ => false,
            }
        }
        ensure!(
            rule.once || branches.iter().all(|event| repeatable(event)),
            "规则 {}：重复 any 只支持时间/变量/类型状态条件，附着对象通知的锁存不能安全逐轮清理",
            rule.id
        );
        let stem = physical(&plan.id, &rule.id, "OR");
        let flag = format!("{stem}_done");
        ensure!(
            used_flags.insert(flag.clone()),
            "any 辅助标记与已有定义冲突：{flag}"
        );
        value["flags"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":flag,"initial":false}));
        let flag_ref = json!({"kind":"flag","id":flag});
        value["rules"][index]["when"] = json!({"kind":"flagSet","flag":flag_ref});
        if !rule.once {
            value["rules"][index]["actions"]
                .as_array_mut()
                .unwrap()
                .push(json!({"kind":"clearFlag","flag":flag_ref}));
        }
        let mut ids = Vec::new();
        for (ordinal, event) in branches.into_iter().enumerate() {
            let id = format!("{stem}_{ordinal}");
            ensure!(
                used_rules.insert(id.clone()),
                "any 辅助规则与已有定义冲突：{id}"
            );
            let mut helper = serde_json::to_value(rule)?;
            helper["id"] = json!(id);
            helper["when"] =
                json!({"kind":"all","events":[{"kind":"flagClear","flag":flag_ref}, event]});
            helper["actions"] = json!([{"kind":"setFlag","flag":flag_ref}]);
            if let Some(origin) = plan
                .origins
                .iter()
                .find(|o| o.kind == "rule" && o.id == rule.id)
            {
                value["origins"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"kind":"rule","id":id,"module":origin.module}));
            }
            helpers.push(helper);
            ids.push(id);
        }
        branches_by_rule.insert(rule.id.clone(), ids);
    }
    // A public enable/disable addresses the coordinator and every branch.
    // Re-enabling also resets each branch's own elapsed timer normally.
    for rule in value["rules"].as_array_mut().unwrap() {
        let mut actions = Vec::<Value>::new();
        for action in rule["actions"].as_array().unwrap() {
            if matches!(action["kind"].as_str(), Some("enableRule" | "disableRule"))
                && let Some(branches) = action["rule"]["id"]
                    .as_str()
                    .and_then(|id| branches_by_rule.get(id))
            {
                ensure!(action["rule"]["kind"] == "rule", "any 控制引用应为 rule");
                for branch in branches {
                    let mut expanded = action.clone();
                    expanded["rule"]["id"] = json!(branch);
                    actions.push(expanded);
                }
            }
            actions.push(action.clone());
        }
        rule["actions"] = Value::Array(actions);
    }
    value["rules"].as_array_mut().unwrap().extend(helpers);
    Ok(Some(serde_json::from_value(value)?))
}
