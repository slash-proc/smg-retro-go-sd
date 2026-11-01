//! Recipe and residual data shipped with this converter, unpacked at build
//! time so the browser WASM does not need an LZMA decoder or temporary files.

pub struct EmbeddedPack {
    pub name: &'static str,
    pub data: &'static [u8],
    pub entries: &'static [(&'static str, usize, usize)],
}

include!(concat!(env!("OUT_DIR"), "/embedded_packs.rs"));
pub static BAKE_AA64: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bake_aa64.bin"));
pub static ASSET_CONFIG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/asset_config.txt"));
pub static ASSET_PALETTE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/asset_palette.rgba"));

/// Get one named item from a project recipe pack.
pub fn get(pack_name: &str, item_name: &str) -> Option<&'static [u8]> {
    let pack = EMBEDDED_PACKS.iter().find(|pack| pack.name == pack_name)?;
    let (_, offset, size) = pack
        .entries
        .iter()
        .find(|(name, _, _)| *name == item_name)?;
    pack.data.get(*offset..offset.checked_add(*size)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_mario_figure_recipe_index_is_available() {
        let meta = get("mario_figure/mario_figure.pak", "meta.json").unwrap();
        assert_eq!(meta.first(), Some(&b'{'));
        assert!(EMBEDDED_PACKS
            .iter()
            .any(|pack| pack.name == "stages/honeyhive.pak"));
    }

    #[test]
    fn every_embedded_pack_entry_is_in_bounds() {
        for pack in EMBEDDED_PACKS {
            for (name, offset, size) in pack.entries {
                let end = offset.checked_add(*size).expect("entry range overflow");
                assert!(
                    end <= pack.data.len(),
                    "{}:{} out of bounds",
                    pack.name,
                    name
                );
            }
        }
    }
}
