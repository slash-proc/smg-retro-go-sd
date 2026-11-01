use std::{env, fs, path::Path};

use smg_iso_verifier::stage_recipe::StageRecipe;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 4 {
        return Err(
            "usage: stage_recipe_probe <embedded-pack> <candidate-json> <output-dir>".into(),
        );
    }
    let recipe = StageRecipe::load(&args[1]).map_err(|error| format!("{error:?}"))?;
    let candidates: serde_json::Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    fs::create_dir_all(&args[3])?;
    for file in &recipe.files {
        // OBJ lines use the same residual streams but also contain compacted
        // vertex/face streams and need the stage geometry pool.
        if file.name.ends_with(".obj") {
            continue;
        }
        let key = if file.name.ends_with(".mtl") {
            "object"
        } else {
            "galaxy"
        };
        let lines = candidates
            .get(key)
            .and_then(serde_json::Value::as_array)
            .ok_or("candidate JSON is missing a string array")?
            .iter()
            .map(|candidate| {
                candidate
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("candidate is not a string")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let decoded = recipe
            .decode_file(&file.name, &lines)
            .map_err(|error| format!("{}: {error:?}", file.name))?;
        fs::write(
            Path::new(&args[3]).join(&file.name),
            decoded.join("\n").as_bytes(),
        )?;
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
