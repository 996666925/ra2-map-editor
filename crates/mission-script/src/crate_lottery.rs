//! Map-local lottery rules. Recruitment and damage remain ordinary Team scripts;
//! the existing crate owner performs placement, random draws and reward delivery.
use super::*;
use vera20k_core::rules::powerups::POWERUP_NAMES;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CrateLotteryRules {
    pub ticket: Reference,
    pub rewards: Vec<Reference>,
    #[serde(default)]
    pub cost: Option<u32>,
}

// Only these rule fields may be restored from a mission ownership manifest.
// Type sections may live in retail rules rather than in the map's VehicleTypes.
pub(super) fn is_managed_field(section: &str, key: &str) -> bool {
    match section {
        "Powerups" => POWERUP_NAMES.contains(&key),
        "CrateRules" => key == "UnitCrateType",
        _ => {
            !section.is_empty()
                && section.len() <= 63
                && section
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && matches!(
                    key,
                    "CrateGoodie" | "CarriesCrate" | "IsTrain" | "Crewed" | "Explodes" | "Cost"
                )
        }
    }
}

pub(super) fn write(
    options: &CrateLotteryRules,
    doc: &MapDocument,
    catalog: &Catalog,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    let vehicles = catalog
        .registry_order
        .get("vehicles")
        .context("crateLottery 需要车辆原生目录，请重新加载资源并刷新 SDK 类型")?;
    let vehicle = |reference: &Reference| -> Result<String> {
        let id = reference.check("unit")?;
        ensure!(
            catalog.units.iter().any(|unit| unit == id)
                && vehicles
                    .iter()
                    .filter(|name| name.eq_ignore_ascii_case(id))
                    .count()
                    == 1,
            "crateLottery {id} 须为当前目录中的唯一车辆类型（不接受步兵或飞机）"
        );
        ensure!(
            is_managed_field(id, "CrateGoodie"),
            "crateLottery 车辆 ID 不能作为规则 section：{id}"
        );
        Ok(id.into())
    };
    let ticket = vehicle(&options.ticket)?;
    ensure!(
        !options.rewards.is_empty() && options.rewards.len() <= 256,
        "crateLottery 奖励车辆池须有 1～256 项"
    );
    let mut rewards = BTreeSet::new();
    for reference in &options.rewards {
        let id = vehicle(reference)?;
        ensure!(!id.eq_ignore_ascii_case(&ticket), "抽奖车不能放进奖励池");
        ensure!(
            rewards.insert(id.to_ascii_lowercase()),
            "crateLottery 奖励车辆重复：{id}"
        );
    }
    if let Some(cost) = options.cost {
        ensure!(
            cost > 0 && cost <= i32::MAX as u32,
            "crateLottery cost 须为 1～2147483647 credits"
        );
        put(&ticket, "Cost", cost.to_string())?;
    }
    for (key, value) in [
        ("CarriesCrate", "yes"),
        ("IsTrain", "no"),
        ("Crewed", "no"),
        ("Explodes", "no"),
    ] {
        put(&ticket, key, value.into())?;
    }
    // Preserve registry order; never register new types or weapons. Only the
    // first Powerups token is written, retaining each live animation/flag/value.
    for name in POWERUP_NAMES {
        let original = doc.ini_text("Powerups", name);
        let suffix = original.find(',').map_or("", |at| &original[at..]);
        put(
            "Powerups",
            name,
            format!("{}{suffix}", if name == "Unit" { "1" } else { "0" }),
        )?;
    }
    put("CrateRules", "UnitCrateType", "<none>".into())?;
    let mut seen = BTreeSet::new();
    for id in vehicles {
        ensure!(
            is_managed_field(id, "CrateGoodie") && seen.insert(id.to_ascii_lowercase()),
            "crateLottery 车辆目录有空项、无效名称或重复项，请修正规则：{id}"
        );
        put(
            id,
            "CrateGoodie",
            if rewards.contains(&id.to_ascii_lowercase()) {
                "yes"
            } else {
                "no"
            }
            .into(),
        )?;
    }
    Ok(())
}
