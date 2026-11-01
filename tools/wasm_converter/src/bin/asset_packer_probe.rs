use smg_iso_verifier::asset_packer::PackedAssets;
use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(
        args.next()
            .ok_or("usage: asset_packer_probe INPUT OUTPUT")?,
    );
    let output = PathBuf::from(
        args.next()
            .ok_or("usage: asset_packer_probe INPUT OUTPUT")?,
    );
    let mode = args.next().and_then(|arg| arg.into_string().ok());
    let already_stage_packed =
        matches!(mode.as_deref(), Some("--stage-packed" | "--overlap-packed"));
    let already_overlap_packed = mode.as_deref() == Some("--overlap-packed");
    if args.next().is_some() {
        return Err(
            "usage: asset_packer_probe INPUT OUTPUT [--stage-packed|--overlap-packed]".into(),
        );
    }
    fs::create_dir_all(&output)?;
    let mut blobs = BTreeMap::new();
    for entry in fs::read_dir(&input)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "bin") {
            blobs.insert(
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read(path)?,
            );
        }
    }
    let mut assets = PackedAssets {
        text: fs::read_to_string(input.join("assets.s"))?,
        header: fs::read_to_string(input.join("assets_gen.h"))?,
        blobs,
    };
    if !already_stage_packed {
        assets.share_tico_bank()?;
        assets.share_canonical_stage_arrays()?;
    }
    if !already_overlap_packed {
        assets.share_canonical_pose_descriptors()?;
        assets.share_canonical_texture_overlaps()?;
    }
    assets.share_canonical_whole_arrays()?;
    assets.pool_canonical_textures()?;
    assets.reanchor_canonical_poses()?;
    let assembled = smg_iso_verifier::asset_assembler::assemble(&assets.text, 0, |name| {
        assets
            .blobs
            .get(name)
            .cloned()
            .ok_or_else(|| format!("missing compiler blob {name}"))
    })?;
    fs::write(output.join("asset_layout.bin"), assembled)?;
    fs::write(output.join("assets.s"), assets.text)?;
    fs::write(output.join("assets_gen.h"), assets.header)?;
    for (name, bytes) in assets.blobs {
        fs::write(output.join(name), bytes)?;
    }
    for entry in fs::read_dir(&input)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "bin")
            || matches!(
                entry.file_name().to_str(),
                Some("assets.s" | "assets_gen.h")
            )
        {
            continue;
        }
        if path.is_file() {
            fs::copy(&path, output.join(entry.file_name()))?;
        }
    }
    Ok(())
}
