#[path = "../asset_assembler.rs"]
mod asset_assembler;

use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let assets_dir = PathBuf::from(
        args.next()
            .ok_or("usage: assemble_assets ASSETS_DIR OUTPUT")?,
    );
    let output = PathBuf::from(
        args.next()
            .ok_or("usage: assemble_assets ASSETS_DIR OUTPUT")?,
    );
    if args.next().is_some() {
        return Err("usage: assemble_assets ASSETS_DIR OUTPUT".into());
    }

    let assembly = fs::read_to_string(assets_dir.join("assets.s"))?;
    let bytes = asset_assembler::assemble(&assembly, 0xBEEF_0000, |name| {
        fs::read(assets_dir.join(name)).map_err(|error| format!("{}: {error}", name))
    })?;
    fs::write(&output, &bytes)?;
    println!("assembled {} bytes into {}", bytes.len(), output.display());
    Ok(())
}
