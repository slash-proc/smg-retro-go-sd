use sha2::{Digest, Sha256};
use smg_iso_verifier::{asset_formats, j3d_model::J3dModel};
use std::{env, fs};

fn print_float_array<const N: usize>(attribute: u32, values: &[[f64; N]]) {
    let mut digest = Sha256::new();
    for value in values.iter().flatten() {
        digest.update(value.to_le_bytes());
    }
    println!(
        "VTX1 {attribute}\t{} values\t{:x}",
        values.len(),
        digest.finalize()
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 {
        return Err("usage: j3d_probe ARCHIVE MODEL_FILE".into());
    }
    let archive = fs::read(&args[1])?;
    let model_data = asset_formats::rarc_find(&archive, &args[2])
        .map_err(|error| format!("RARC parse failed: {error:?}"))?;
    let model =
        J3dModel::parse(&model_data).map_err(|error| format!("J3D parse failed: {error:?}"))?;
    let vertices: usize = model.shapes.iter().map(|shape| shape.vertices.len()).sum();
    let triangles: usize = model.shapes.iter().map(|shape| shape.triangles.len()).sum();
    let material_shapes = model
        .shapes
        .iter()
        .filter(|shape| shape.material.is_some())
        .count();
    let normals = model.normals.get(&10).or_else(|| model.normals.get(&25));
    println!(
        "vertex arrays\t{} positions\t{} normals\t{} colors\tUV sets {:?}",
        model.positions.len(),
        normals.map_or(0, Vec::len),
        model.colors.len(),
        model.tex_coords.iter().map(Vec::len).collect::<Vec<_>>(),
    );
    print_float_array(9, &model.positions);
    for (&attribute, values) in &model.normals {
        print_float_array(attribute, values);
    }
    for (slot, values) in model.tex_coords.iter().enumerate() {
        print_float_array(13 + slot as u32, values);
    }
    let mut color_digest = Sha256::new();
    for color in &model.colors {
        color_digest.update(color);
    }
    println!(
        "VTX1 11\t{} values\t{:x}",
        model.colors.len(),
        color_digest.finalize()
    );
    println!(
        "{}\t{} joints\t{} shapes\t{} material shapes\t{} vertices\t{} triangles",
        args[2],
        model.joints.len(),
        model.shapes.len(),
        material_shapes,
        vertices,
        triangles
    );
    for (shape_index, shape) in model.shapes.iter().enumerate() {
        let mut digest = Sha256::new();
        for vertex in &shape.vertices {
            digest.update((vertex.draw_matrix as u32).to_le_bytes());
            digest.update((vertex.attributes.len() as u32).to_le_bytes());
            for (&attribute, &index) in &vertex.attributes {
                digest.update(attribute.to_le_bytes());
                digest.update((index as u32).to_le_bytes());
            }
        }
        for triangle in &shape.triangles {
            for &index in triangle {
                digest.update((index as u32).to_le_bytes());
            }
        }
        println!(
            "shape {}\tmaterial {:?}\t{} vertices\t{} triangles\t{:x}",
            shape_index,
            shape.material,
            shape.vertices.len(),
            shape.triangles.len(),
            digest.finalize()
        );
    }
    Ok(())
}
