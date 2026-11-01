use std::{collections::BTreeMap, env, fs, path::Path};

use serde_json::json;
use smg_iso_verifier::stage_galaxy::StageGalaxy;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: stage_galaxy_probe <StageData-dir> <galaxy>".into());
    }
    let root = Path::new(&args[1]);
    let galaxy = &args[2];
    let scenario_path = root.join(galaxy).join(format!("{galaxy}Scenario.arc"));
    let scenario = fs::read(scenario_path)?;
    let mut zones = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "arc") {
            let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            zones.insert(name.to_owned(), fs::read(path)?);
        }
    }
    let stage = StageGalaxy::parse(galaxy, &scenario, &zones)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let output = json!({
        "galaxy": stage.galaxy,
        "zone_list": stage.zone_list,
        "placements": stage.placements.iter().map(|placement| json!({
            "zone": placement.zone,
            "parent": placement.parent,
            "matrix": placement.matrix,
            "raw": placement.raw,
            "scenarios": placement.scenarios,
        })).collect::<Vec<_>>(),
        "objects": stage.objects.iter().map(|object| json!({
            "index": object.index,
            "name": object.name,
            "zone": object.zone,
            "zone_placement": object.zone_placement,
            "world_pos": object.world_pos,
            "world_rot": object.world_rot,
            "world_scale": object.world_scale,
            "raw": object.raw,
            "scenarios": object.scenarios,
        })).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
