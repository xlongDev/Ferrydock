use crate::error::Result;
use crate::router::OWNER;
use serde_json::Value;
use std::path::Path;

/// Reconcile the WorkBuddy models.json: remove entries we previously owned
/// (tagged with OWNER) and append the current `entries`. Writes atomically and
/// keeps a timestamped backup of the previous file.
pub fn sync_workbuddy(path: &Path, entries: &[Value]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut arr: Vec<Value> = if path.exists() {
        let txt = std::fs::read_to_string(path)?;
        serde_json::from_str(&txt).unwrap_or_else(|_| Vec::new())
    } else {
        Vec::new()
    };

    backup(path)?;

    // Drop our previously-owned entries, then append the fresh set.
    arr.retain(|m| m.get("ocFerryOwner").and_then(|o| o.as_str()) != Some(OWNER));
    for e in entries {
        arr.push(e.clone());
    }

    let txt = serde_json::to_string_pretty(&arr)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, txt)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn backup(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "models.json".into());
    let bak_name = format!("{name}.ferrydock-{ts}.bak");
    let bak = path
        .parent()
        .map(|p| p.join(&bak_name))
        .unwrap_or_else(|| Path::new(&bak_name).to_path_buf());
    std::fs::copy(path, &bak)?;
    Ok(())
}
