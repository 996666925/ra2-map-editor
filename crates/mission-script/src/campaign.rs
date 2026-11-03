//! Declarative campaign entry, normalized to the existing publication model.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Campaign {
    pub project: ra2_map_document::campaign::Project,
    #[serde(default)]
    pub scripts: BTreeMap<String, String>,
    #[serde(default)]
    pub transitions: BTreeMap<String, Transition>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transition {
    pub next: Option<String>,
    pub alternate: Option<String>,
    #[serde(default)]
    pub end_of_game: bool,
}
impl Campaign {
    pub fn validate(&self) -> Result<()> {
        self.project.validate()?;
        for path in self
            .project
            .missions
            .iter()
            .map(|m| &m.source)
            .chain(self.project.assets.iter().map(|a| &a.source))
            .chain(self.scripts.values())
        {
            ensure!(
                !path.is_empty()
                    && path.len() <= 1024
                    && !path.contains(['\\', ':', '\0'])
                    && path.split('/').all(|p| !matches!(p, "" | "." | "..")),
                "战役依赖须为入口目录内相对路径：{path}"
            );
        }
        for file in self.scripts.keys().chain(self.transitions.keys()) {
            ensure!(
                self.project.missions.iter().any(|m| &m.file == file),
                "战役引用未知关卡：{file}"
            );
        }
        for transition in self.transitions.values() {
            ensure!(
                if transition.end_of_game {
                    transition.next.is_none() && transition.alternate.is_none()
                } else {
                    transition.next.is_some()
                },
                "终关不能有后续目标，非终关必须指定 next"
            );
            for next in transition.next.iter().chain(transition.alternate.iter()) {
                ensure!(
                    self.project
                        .missions
                        .iter()
                        .any(|m| m.file.eq_ignore_ascii_case(next)),
                    "后续关卡未打包：{next}"
                );
            }
        }
        Ok(())
    }
}
