use smg_iso_verifier::{
    asset_formats, asset_packs,
    geometry_bake::{
        build_bake_materials, parse_obj, prepare_high_mesh_with_options, serialize_bake_job,
        BakeOptions,
    },
    j3d_model::J3dModel,
    tev::TevModel,
};
use std::{env, fs};

fn aa64_table() -> Result<Vec<[f32; 2]>, Box<dyn std::error::Error>> {
    let packed = asset_packs::BAKE_AA64;
    let mut values = Vec::with_capacity(packed.len() * 4 / 3);
    for rgb in packed.chunks_exact(3) {
        let bits = ((rgb[0] as u32) << 16) | ((rgb[1] as u32) << 8) | rgb[2] as u32;
        values.extend_from_slice(&[
            (bits >> 18) as u8,
            ((bits >> 12) & 63) as u8,
            ((bits >> 6) & 63) as u8,
            (bits & 63) as u8,
        ]);
    }
    let mut offsets = Vec::with_capacity(64 * 64 * 4);
    for pixel in 0..64 * 64 {
        offsets.push([0.0, 0.0]);
        for sample in 0..3 {
            let start = pixel * 6 + sample * 2;
            offsets.push([
                (values[start] as f32 + 0.5) / 64.0 - 0.5,
                (values[start + 1] as f32 + 0.5) / 64.0 - 0.5,
            ]);
        }
    }
    Ok(offsets)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if !(5..=8).contains(&args.len()) {
        return Err("usage: bake_job_probe ARCHIVE MODEL_FILE LOW_OBJ OUTPUT [BAKE_SIZE=64] [FIX=0|1] [PARTS=0|1]".into());
    }
    let bake_size = args.get(5).map_or(Ok(64usize), |s| s.parse())?;
    let fix = args.get(6).is_some_and(|s| s == "1");
    let parts = args.get(7).is_some_and(|s| s == "1");
    let archive = fs::read(&args[1])?;
    let body = asset_formats::rarc_find(&archive, &args[2])
        .map_err(|e| format!("RARC parse failed: {e:?}"))?;
    let model = J3dModel::parse(&body).map_err(|e| format!("J3D parse failed: {e:?}"))?;
    let tev = TevModel::parse(&body).map_err(|e| format!("TEV parse failed: {e:?}"))?;
    let high = prepare_high_mesh_with_options(&model, &tev, fix, &[])
        .map_err(|e| format!("high mesh failed: {e:?}"))?;
    let low =
        parse_obj(&fs::read(&args[3])?, None).map_err(|e| format!("OBJ parse failed: {e:?}"))?;
    let (materials, textures) = build_bake_materials(&tev, &high, false, None)
        .map_err(|e| format!("material build failed: {e:?}"))?;
    let aa = if !parts && bake_size == 64 {
        aa64_table()?
    } else {
        Vec::new()
    };
    let payload = serialize_bake_job(
        &low,
        &high,
        &materials,
        &textures,
        BakeOptions {
            width: if parts { 64 } else { bake_size },
            height: if parts { 64 } else { bake_size },
            margin: if parts { 0 } else { 4 },
            flags: if parts { 65_536 } else { 0 },
            extent_factor: 0.06,
        },
        &aa,
    )
    .map_err(|e| format!("job serialization failed: {e:?}"))?;
    fs::write(&args[4], &payload)?;
    println!(
        "{} bytes; bake {}{}; low {} verts/{} polygons; high {} verts/{} tris/{} mats; {} texs",
        payload.len(),
        bake_size,
        if parts { " parts" } else { "" },
        low.positions.len(),
        low.faces.len(),
        high.positions.len(),
        high.triangles.len(),
        materials.len(),
        textures.len()
    );
    Ok(())
}
