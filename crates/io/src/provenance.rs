//! Sidecars record the actual map, schema, resources and executable used.
use anyhow::Result;
use ra2_map_assets::profile::{self, Revision};
use std::path::{Path, PathBuf};

pub fn sidecar(map: &Path) -> PathBuf {
    let mut path = map.as_os_str().to_os_string();
    path.push(".yr-editor.json");
    path.into()
}
pub fn write(path: &Path, bytes: &[u8], revision: &Revision, exe: Option<&Path>) -> Result<()> {
    let executable = exe
        .map(|exe| -> Result<_> {
            Ok(serde_json::json!({"path":exe,"hash":format!("{:016x}",profile::file_hash(exe)?)}))
        })
        .transpose()?;
    let record = serde_json::json!({"version":1,"editor":env!("CARGO_PKG_VERSION"),"map_hash":format!("{:016x}",profile::hash(bytes)),"resource_profile":revision,"executable":executable});
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temp, serde_json::to_vec_pretty(&record)?)?;
    std::fs::rename(temp, path)?;
    Ok(())
}
