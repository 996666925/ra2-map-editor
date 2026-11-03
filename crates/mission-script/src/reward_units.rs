//! Reward-only VehicleTypes are independent map definitions. The engine's normal
//! rules loader and lottery teams consume them; original type sections stay intact.
use super::*;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewardUnitDefinition {
    pub id: String,
    pub base: Reference,
    #[serde(default)]
    pub ui_name: Option<Reference>,
    pub rules: BTreeMap<String, serde_json::Value>,
}

pub(super) fn generated_id(id: &str) -> bool {
    id.len() == 20 && id.starts_with("TSRU") && id[4..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn value_text(value: &serde_json::Value) -> Result<String> {
    match value {
        serde_json::Value::String(text) => Ok(text.clone()),
        serde_json::Value::Bool(value) => Ok(if *value { "yes" } else { "no" }.into()),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        _ => anyhow::bail!("奖励属性只接受字符串、有限数字或 boolean"),
    }
}

pub(super) fn write(
    plan: &Mission,
    doc: &MapDocument,
    catalog: &Catalog,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<BTreeMap<String, String>> {
    ensure!(plan.reward_units.len() <= 64, "奖励专用类型最多 64 项");
    let mut ids = BTreeMap::new();
    let vehicles = catalog
        .registry_order
        .get("vehicles")
        .context("缺少车辆原生目录，请刷新 SDK 类型")?;
    for unit in &plan.reward_units {
        identifier(&unit.id)?;
        let base = unit.base.check("unit")?;
        ensure!(
            vehicles
                .iter()
                .filter(|id| id.eq_ignore_ascii_case(base))
                .count()
                == 1
                && !generated_id(base),
            "奖励副本的 base 须为现有普通车辆：{base}"
        );
        let id = physical(&plan.id, &unit.id, "RU");
        ensure!(
            ids.insert(unit.id.clone(), id.clone()).is_none(),
            "重复奖励类型 {}",
            unit.id
        );
        ensure!(
            !catalog
                .units
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&id))
                && doc.ini_entries(&id).is_empty(),
            "奖励类型 {id} 与现有定义冲突"
        );
        ensure!(
            plan.crate_lottery.as_ref().is_some_and(|lottery| lottery
                .rewards
                .iter()
                .any(|r| r.kind == "rewardUnit" && r.id == unit.id)),
            "奖励类型 {} 未加入抽奖池",
            unit.id
        );
        let mut fields = catalog
            .vehicle_templates
            .get(base)
            .cloned()
            .with_context(|| format!("车辆 {base} 缺少宿主规则快照，请重新加载地图资源"))?;
        // Include current map-local edits after restoring previous managed rows.
        for (key, value) in doc.ini_fields(base) {
            if value.is_empty() {
                fields.remove(&key);
            } else {
                fields.insert(key, String::from_utf8(value).context("车辆规则须为 UTF-8")?);
            }
        }
        fields.entry("Image".into()).or_insert_with(|| base.into());
        ensure!(unit.rules.len() <= 512, "奖励属性超过 512 项");
        for (key, value) in &unit.rules {
            ensure!(
                !key.is_empty()
                    && key.len() <= 127
                    && key
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.'),
                "无效奖励属性名 {key:?}"
            );
            ensure!(
                ![
                    "TechLevel",
                    "CrateGoodie",
                    "AllowedToStartInMultiplayer",
                    "UIName"
                ]
                .iter()
                .any(|reserved| key.eq_ignore_ascii_case(reserved)),
                "奖励隔离字段 {key} 由 SDK 管理"
            );
            ensure!(
                !fields
                    .keys()
                    .any(|old| old.eq_ignore_ascii_case(key) && old != key),
                "属性 {key} 大小写与基础规则不同"
            );
            let text = value_text(value)?;
            ensure!(
                !text.is_empty() && text.len() <= 480 && !text.contains(['\n', '\r', '\0', ';']),
                "奖励属性 {key} 为空、过长或包含 INI 控制字符；清空引用请用 <none>"
            );
            let numbered_weapon = ["Weapon", "EliteWeapon"].iter().any(|prefix| {
                key.strip_prefix(prefix).is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                })
            });
            if matches!(
                key.as_str(),
                "Primary"
                    | "Secondary"
                    | "ElitePrimary"
                    | "EliteSecondary"
                    | "DeathWeapon"
                    | "OccupyWeapon"
                    | "EliteOccupyWeapon"
            ) || numbered_weapon
            {
                ensure!(
                    text.eq_ignore_ascii_case("<none>")
                        || text.eq_ignore_ascii_case("none")
                        || catalog
                            .weapons
                            .iter()
                            .any(|weapon| weapon.eq_ignore_ascii_case(&text)),
                    "奖励武器 {text} 须已存在于当前资源目录，不能在副本中隐式创建武器"
                );
            }
            fields.insert(key.clone(), text);
        }
        fields.insert("TechLevel".into(), "-1".into());
        fields.insert("CrateGoodie".into(), "no".into());
        fields.insert("AllowedToStartInMultiplayer".into(), "no".into());
        if let Some(label) = &unit.ui_name {
            let label = label.check("text")?;
            ensure!(
                catalog.strings.iter().any(|id| id == label),
                "奖励名称标签不存在"
            );
            fields.insert("UIName".into(), label.into());
        }
        put("VehicleTypes", &id, id.clone())?;
        // Prize-only variants must never enter ordinary random crate pools.
        for (key, value) in fields {
            if !value.is_empty() {
                put(&id, &key, value)?;
            }
        }
    }
    Ok(ids)
}
