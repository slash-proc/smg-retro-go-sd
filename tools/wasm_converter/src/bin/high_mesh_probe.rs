use sha2::{Digest, Sha256};
use smg_iso_verifier::{
    asset_formats,
    geometry_bake::{
        parse_obj, prepare_high_mesh, prepare_high_mesh_with_options,
        prepare_high_mesh_with_pose_and_visibility, unpose_low_vertices,
    },
    j3d_animation::{fitted_world, Bck},
    j3d_model::J3dModel,
    tev::TevModel,
};
use std::{env, fs};

fn digest_f64<const N: usize>(values: &[[f64; N]]) -> String {
    let mut digest = Sha256::new();
    for value in values.iter().flatten() {
        digest.update(value.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn digest_u32<const N: usize>(values: &[[u32; N]]) -> String {
    let mut digest = Sha256::new();
    for value in values.iter().flatten() {
        digest.update(value.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if !(3..=4).contains(&args.len()) && args.len() != 7 && args.len() != 9 {
        return Err("usage: high_mesh_probe ARCHIVE MODEL_FILE [fix | pose BCK_ARCHIVE BCK_FILE FRAME [unpose OBJ]]".into());
    }
    let fix = args.get(3).is_some_and(|arg| arg == "fix");
    let archive = fs::read(&args[1])?;
    let body = asset_formats::rarc_find(&archive, &args[2])
        .map_err(|e| format!("RARC parse failed: {e:?}"))?;
    let model = J3dModel::parse(&body).map_err(|e| format!("J3D parse failed: {e:?}"))?;
    let tev = TevModel::parse(&body).map_err(|e| format!("TEV parse failed: {e:?}"))?;
    let mut sampled_pose = None;
    let high = if args.get(3).is_some_and(|arg| arg == "pose") {
        let animation_archive = fs::read(&args[4])?;
        let animation = asset_formats::rarc_find(&animation_archive, &args[5])
            .map_err(|e| format!("BCK RARC parse failed: {e:?}"))?;
        let bck = Bck::parse(&animation).map_err(|e| format!("BCK parse failed: {e:?}"))?;
        let frame = args[6].parse::<f64>()?;
        let raw = bck.raw(&model, frame);
        let mut raw_digest = Sha256::new();
        for joint in &raw {
            for value in joint
                .scale
                .into_iter()
                .chain(joint.rotation)
                .chain(joint.translation)
            {
                raw_digest.update(value.to_le_bytes());
            }
        }
        println!("RAW {:x}", raw_digest.finalize());
        let pose = fitted_world(&model, &raw);
        println!("POSE {}", digest_f64(&pose));
        sampled_pose = Some(pose.clone());
        prepare_high_mesh_with_pose_and_visibility(&model, &tev, false, &[], Some(&pose), None)
    } else if fix {
        prepare_high_mesh_with_options(&model, &tev, true, &[])
    } else {
        prepare_high_mesh(&model, &tev)
    }
    .map_err(|e| format!("mesh prepare failed: {e:?}"))?;
    if args.get(7).is_some_and(|arg| arg == "unpose") {
        let bytes = fs::read(&args[8])?;
        let mut low = parse_obj(&bytes, None).map_err(|e| format!("OBJ parse failed: {e:?}"))?;
        unpose_low_vertices(
            &model,
            &high,
            sampled_pose
                .as_deref()
                .ok_or("unpose requires a sampled pose")?,
            &mut low.positions,
        )
        .map_err(|e| format!("unpose failed: {e:?}"))?;
        println!(
            "LOW_UNPOSE {} {}",
            low.positions.len(),
            digest_f64(&low.positions)
        );
    }
    println!(
        "vertices {} triangles {} materials {}",
        high.positions.len(),
        high.triangles.len(),
        high.materials.len()
    );
    println!("P {}", digest_f64(&high.positions));
    println!(
        "D {}",
        digest_u32(
            &high
                .draw_matrices
                .iter()
                .map(|&value| [value as u32])
                .collect::<Vec<_>>()
        )
    );
    println!("UV {}", digest_f64(&high.uvs));
    println!("C {}", digest_f64(&high.colors));
    println!("N {}", digest_f64(&high.normals));
    println!("T {}", digest_u32(&high.triangles));
    println!(
        "M {}",
        digest_u32(
            &high
                .triangle_materials
                .iter()
                .map(|&v| [v])
                .collect::<Vec<_>>()
        )
    );
    Ok(())
}
