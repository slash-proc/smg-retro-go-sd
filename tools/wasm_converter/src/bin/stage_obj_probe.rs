use std::{collections::BTreeMap, env, fs, path::Path};

use smg_iso_verifier::{stage_assets, stage_galaxy::StageGalaxy, stage_recipe::StageRecipe};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 4 {
        return Err(
            "usage: stage_obj_probe <orig-files-dir> <embedded-stage-pack> <output-dir>".into(),
        );
    }
    let root = Path::new(&args[1]);
    let output = Path::new(&args[3]);
    let recipe = StageRecipe::load(&args[2]).map_err(|error| format!("recipe: {error:?}"))?;
    let stage_data = root.join("StageData");
    let scenario = fs::read(
        stage_data
            .join(&recipe.galaxy)
            .join(format!("{}Scenario.arc", recipe.galaxy)),
    )?;
    let mut zone_archives = BTreeMap::new();
    for entry in fs::read_dir(&stage_data)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "arc") {
                if let Some(name) = path.file_stem().and_then(|name| name.to_str()) {
                    zone_archives.insert(name.to_owned(), fs::read(path)?);
                }
            }
        }
    }
    let stage = StageGalaxy::parse(&recipe.galaxy, &scenario, &zone_archives)
        .map_err(|error| format!("stage: {error:?}"))?;
    fs::create_dir_all(output)?;
    let generated =
        stage_assets::geometry_files(&recipe, &stage, |path| fs::read(root.join(path)).ok())
            .map_err(|error| format!("stage assets: {error:?}"))?;
    for (path, bytes) in generated {
        let name = Path::new(&path).file_name().ok_or("invalid output path")?;
        fs::write(output.join(name), &bytes)?;
        println!("{}: {} bytes", name.to_string_lossy(), bytes.len());
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
