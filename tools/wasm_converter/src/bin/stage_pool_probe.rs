use std::{collections::BTreeMap, env, fs, path::Path};

use sha2::{Digest, Sha256};
use smg_iso_verifier::{stage_galaxy::StageGalaxy, stage_geometry, stage_recipe::StageRecipe};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 3 && args.len() != 4 {
        return Err(
            "usage: stage_pool_probe <orig-files-dir> <embedded-stage-pack> [pool-json]".into(),
        );
    }
    let root = Path::new(&args[1]);
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
    let mut count = 0;
    let mut uv_count = 0;
    let mut hash = Sha256::new();
    let mut all_positions = Vec::new();
    let mut all_uvs = Vec::new();
    for source in &recipe.sources {
        let pool = stage_geometry::build_pool(&stage, std::slice::from_ref(source), |name| {
            fs::read(root.join("ObjectData").join(format!("{name}.arc"))).ok()
        })
        .map_err(|error| format!("{}[{}]: {error:?}", source.archive, source.object_index))?;
        let mut source_hash = Sha256::new();
        let mut position_hash = Sha256::new();
        let mut uv_hash = Sha256::new();
        for (position, uvs) in pool.positions.iter().zip(&pool.uvs) {
            for value in position {
                let bytes = value.to_be_bytes();
                hash.update(bytes);
                source_hash.update(bytes);
                position_hash.update(bytes);
            }
            let length = (uvs.len() as u64).to_be_bytes();
            hash.update(length);
            source_hash.update(length);
            uv_hash.update(length);
            for uv in uvs {
                let u = uv[0].to_be_bytes();
                let v = uv[1].to_be_bytes();
                hash.update(u);
                hash.update(v);
                source_hash.update(u);
                source_hash.update(v);
                uv_hash.update(u);
                uv_hash.update(v);
            }
        }
        let source_digest = source_hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let position_digest = position_hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let uv_digest = uv_hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        println!(
            "{}[{}]: {} points, sha256 {} positions {} uvs {}",
            source.archive,
            source.object_index,
            pool.positions.len(),
            source_digest,
            position_digest,
            uv_digest
        );
        count += pool.positions.len();
        uv_count += pool.uvs.iter().map(Vec::len).sum::<usize>();
        if args.len() == 4 {
            all_positions.extend(pool.positions);
            all_uvs.extend(pool.uvs);
        }
    }
    let digest = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    println!("{} positions, {} UVs, sha256 {}", count, uv_count, digest);
    if let Some(path) = args.get(3) {
        fs::write(
            path,
            serde_json::to_vec(&serde_json::json!({
                "positions": all_positions,
                "uvs": all_uvs,
            }))?,
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
