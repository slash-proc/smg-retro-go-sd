use std::{env, fs};

use serde_json::json;
use smg_iso_verifier::{asset_formats, stage_geometry};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 3 && args.len() != 4 {
        return Err("usage: stage_geometry_probe <archive> <model-path> [kcl-path]".into());
    }
    let archive = fs::read(&args[1])?;
    let model_bytes = asset_formats::rarc_find(&archive, &args[2])
        .map_err(|error| format!("model archive: {error:?}"))?;
    let model =
        stage_geometry::model_points(&model_bytes).map_err(|error| format!("model: {error:?}"))?;
    let points = if args.len() == 4 {
        let collision_bytes = asset_formats::rarc_find(&archive, &args[3])
            .map_err(|error| format!("collision archive: {error:?}"))?;
        let triangles = stage_geometry::kcl_triangles(&collision_bytes)
            .map_err(|error| format!("collision: {error:?}"))?;
        stage_geometry::kcl_points(&triangles)
    } else {
        Vec::new()
    };
    println!(
        "{}",
        serde_json::to_string(&json!({
            "positions": model.positions,
            "uvs": model.uvs,
            "kcl_points": points,
        }))?
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
