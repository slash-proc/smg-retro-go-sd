//! Fixed outputs from the version-locked UI/text construction pipeline.
//!
//! The canonical C asset compiler consumes these relative to its asset root.

pub fn generate() -> Vec<(String, Vec<u8>)> {
    let pack = crate::asset_packs::EMBEDDED_PACKS
        .iter()
        .find(|pack| pack.name == "ui_headers/ui_headers.pak")
        .expect("embedded UI asset inputs");
    let mut files = Vec::new();
    for (name, offset, size) in pack.entries {
        let Some(path) = name.strip_prefix("asset:") else {
            continue;
        };
        let end = offset.checked_add(*size).expect("UI asset range overflow");
        let bytes = pack.data.get(*offset..end).expect("UI asset range");
        files.push((format!("assets/{path}"), bytes.to_vec()));
    }
    files
}
