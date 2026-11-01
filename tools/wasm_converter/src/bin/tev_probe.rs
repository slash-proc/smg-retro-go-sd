use sha2::{Digest, Sha256};
use smg_iso_verifier::{asset_formats, tev::TevModel};
use std::{env, fs};

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let archive = fs::read(
        args.next()
            .ok_or("usage: tev_probe ARCHIVE INNER_PATH MATERIAL_INDEX")?,
    )
    .map_err(|error| error.to_string())?;
    let inner = args.next().ok_or("missing inner path")?;
    let material: usize = args
        .next()
        .ok_or("missing material index")?
        .parse()
        .map_err(|error: std::num::ParseIntError| error.to_string())?;
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let model = asset_formats::rarc_find(&archive, &inner).map_err(|error| format!("{error:?}"))?;
    let tev = TevModel::parse(&model).map_err(|error| format!("{error:?}"))?;
    let bake_info = tev
        .bake_material_info(material)
        .map_err(|error| format!("{error:?}"))?;
    println!("bake {bake_info:?}");
    let (width, height, rgba) = tev
        .effective_rgba(material)
        .map_err(|error| format!("{error:?}"))?;
    let digest = Sha256::digest(&rgba);
    println!(
        "{width} {height} {}",
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    Ok(())
}
