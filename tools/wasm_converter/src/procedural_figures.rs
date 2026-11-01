//! Rust port of `asset_construction/procedural.py::figures`.

use crate::asset_formats::{particle_ids, particle_sizes, rarc_find, ArchiveError};

const K: f64 = 0.004;
const JPA_UNIT: f64 = 25.0;
const SECTION: [(f64, f64); 3] = [(10.0, -10.0), (0.0, 10.0), (-10.0, -10.0)];
const CAGE: [(f64, f64); 6] = [
    (-0.24249, 0.14),
    (-0.0, 0.28),
    (0.24249, 0.14),
    (0.24249, -0.14),
    (-0.0, -0.28),
    (-0.24249, -0.14),
];
const CAGE_UV: &str = "67193 33808 99421 64047 67193 65880 67193 33808 99421 35641 99421 64047 \
65880 33964 35641 66192 33808 33964 65880 33964 64047 66192 35641 66192 \
32650 00579 02411 32807 00579 00579 32650 00579 30817 32807 02411 32807 \
99421 32650 67193 02411 99421 00579 99421 32650 67193 30817 67193 02411 \
00579 66192 30817 33964 32650 66192 00579 66192 02411 33964 30817 33964 \
66036 32650 33808 02411 66036 00579 66036 32650 33808 30817 33808 02411";
const TAIL_RADII: [u32; 8] = [85, 79, 70, 62, 55, 49, 43, 43];

fn text(lines: Vec<String>) -> Vec<u8> {
    let mut result = lines.join("\n").into_bytes();
    result.push(b'\n');
    result
}

fn mtl(names: &[(&str, &str)], tab: bool, kd: bool) -> Vec<u8> {
    let mut lines = Vec::new();
    for (name, texture) in names {
        lines.push(format!("newmtl {name}"));
        if kd {
            lines.push("Kd 1 1 1".into());
        }
        lines.push(format!("{}map_Kd {texture}", if tab { "\t" } else { "" }));
    }
    text(lines)
}

fn creeper_stalk() -> (Vec<u8>, Vec<u8>) {
    let mut lines = vec!["mtllib creeperstalk.mtl".to_string()];
    for y in [0.0, 250.0] {
        for (x, z) in SECTION {
            lines.push(format!("v {:.5} {:.5} {:.5}", x * K, y * K, z * K));
        }
    }
    let mut uv = Vec::new();
    let mut faces: Vec<Vec<(usize, usize)>> = Vec::new();
    for k in 0..3 {
        let a = k;
        let b = (k + 1) % 3;
        let base = uv.len();
        uv.extend([(1.0, 0.0), (0.0, 0.0), (0.0, 0.25), (1.0, 0.25)]);
        let q = [
            (a + 1, base + 1),
            (b + 1, base + 2),
            (b + 4, base + 3),
            (a + 4, base + 4),
        ];
        faces.push(vec![q[0], q[2], q[1]]);
        faces.push(vec![q[0], q[3], q[2]]);
    }
    lines.extend(uv.iter().map(|(u, v)| format!("vt {:.5} {:.5}", u, v)));
    lines.push("usemtl creeperstalk_m0".into());
    lines.extend(faces.iter().map(|f| {
        format!(
            "f {}/{} {}/{} {}/{}",
            f[0].0, f[0].1, f[1].0, f[1].1, f[2].0, f[2].1
        )
    }));
    (
        text(lines),
        mtl(&[("creeperstalk_m0", "creeperstalk_m0.png")], true, false),
    )
}

fn plant_stalk() -> (Vec<u8>, Vec<u8>) {
    let front = (0.0, 0.0, 10.0);
    let right = (10.0, 0.0, -10.0);
    let left = (-10.0, 0.0, -10.0);
    let sides = [(front, right), (left, front), (right, left)];
    let mut points: Vec<(f64, f64, f64)> = Vec::new();
    let mut uvs = Vec::new();
    let mut faces = Vec::new();
    let mut materials = Vec::new();
    for (i, (a, b)) in sides.iter().enumerate() {
        let material = format!("plantstalk_m{i}");
        materials.push((material.clone(), format!("{material}.png")));
        let mut quad = Vec::new();
        for (p, y, u) in [
            (*a, 0.0, 0.0),
            (*b, 0.0, 1.0),
            (*b, 100.0, 1.0),
            (*a, 100.0, 0.0),
        ] {
            let q = (p.0, y, p.2);
            let index = points.iter().position(|old| *old == q).unwrap_or_else(|| {
                points.push(q);
                points.len() - 1
            });
            uvs.push((u, 1.0 - y / 30.0));
            quad.push((index + 1, uvs.len()));
        }
        let p0 = points[quad[0].0 - 1];
        let p1 = points[quad[1].0 - 1];
        let p3 = points[quad[3].0 - 1];
        let e1 = [p1.0 - p0.0, p1.1 - p0.1, p1.2 - p0.2];
        let e2 = [p3.0 - p0.0, p3.1 - p0.1, p3.2 - p0.2];
        let nx = e1[1] * e2[2] - e1[2] * e2[1];
        let nz = e1[0] * e2[1] - e1[1] * e2[0];
        if nx * (a.0 + b.0) / 2.0 + nz * (a.2 + b.2) / 2.0 < 0.0 {
            quad.reverse();
        }
        faces.push(format!("usemtl {material}"));
        for tri in [[0, 1, 2], [0, 2, 3]] {
            faces.push(format!(
                "f {}/{} {}/{} {}/{}",
                quad[tri[0]].0,
                quad[tri[0]].1,
                quad[tri[1]].0,
                quad[tri[1]].1,
                quad[tri[2]].0,
                quad[tri[2]].1
            ));
        }
    }
    let mut lines = vec!["mtllib plantstalk.mtl".into()];
    lines.extend(
        points
            .iter()
            .map(|p| format!("v {:.5} {:.5} {:.5}", p.0 * K, p.1 * K, p.2 * K)),
    );
    lines.extend(uvs.iter().map(|(u, v)| format!("vt {:.5} {:.5}", u, v)));
    lines.extend(faces);
    let obj = text(lines);
    let mats = materials
        .iter()
        .map(|(name, tex)| (name.as_str(), tex.as_str()))
        .collect::<Vec<_>>();
    (obj, mtl(&mats, true, false))
}

fn lod_of(obj: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(obj).lines() {
        let tokens = line.split_whitespace().collect::<Vec<_>>();
        if tokens.first().is_some_and(|t| *t == "v" || *t == "vt") {
            let mut rewritten = tokens[0].to_string();
            for token in &tokens[1..] {
                let value: f64 = token.parse().unwrap();
                rewritten.push_str(&format!(" {:.6}", value));
            }
            out.extend_from_slice(rewritten.as_bytes());
        } else {
            out.extend_from_slice(line.as_bytes());
        }
        out.push(b'\n');
    }
    out
}

fn tail_chain() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut lines = vec!["mtllib dinopackuntailchain.mtl".to_string()];
    for (r, radius) in TAIL_RADII.iter().enumerate() {
        for (x, y) in CAGE {
            let n = (x * x + y * y).sqrt();
            let rad = *radius as f64 * K;
            lines.push(format!(
                "v {:.5} {:.5} {:.5}",
                rad * (x / n) + 0.0,
                rad * (y / n) + 0.0,
                r as f64 * 70.0 * K
            ));
        }
    }
    let uv = CAGE_UV.split_whitespace().collect::<Vec<_>>();
    for pair in uv.chunks_exact(2) {
        lines.push(format!("vt 0.{} 0.{}", pair[0], pair[1]));
    }
    lines.push("usemtl dinopackuntailchain_m0".into());
    for s in 0..TAIL_RADII.len() - 1 {
        for k in 0..CAGE.len() {
            let a = s * CAGE.len() + k + 1;
            let b = s * CAGE.len() + (k + 1) % CAGE.len() + 1;
            let t = 6 * k + 1;
            lines.push(format!(
                "f {a}/{t} {}/{t1} {b}/{t2}",
                b + CAGE.len(),
                t1 = t + 1,
                t2 = t + 2
            ));
            lines.push(format!(
                "f {a}/{t3} {}/{t4} {}/{t5}",
                a + CAGE.len(),
                b + CAGE.len(),
                t3 = t + 3,
                t4 = t + 4,
                t5 = t + 5
            ));
        }
    }
    let obj = text(lines);
    let lod = String::from_utf8_lossy(&obj)
        .replace("dinopackuntailchain.mtl", "lod.mtl")
        .into_bytes();
    let mat = mtl(
        &[("dinopackuntailchain_m0", "dinopackuntailchain_m0.png")],
        true,
        false,
    );
    (obj, lod, mat)
}

fn billboard(
    name: &str,
    material: &str,
    texture: &str,
    half: f64,
    precision: Option<usize>,
) -> (Vec<u8>, Vec<u8>) {
    let h = match precision {
        Some(p) => format!("{half:.p$}"),
        None => format!("{half:?}"),
    };
    let mut lines = vec![format!("mtllib {name}.mtl")];
    for (sx, sy) in [(-1, -1), (1, -1), (1, 1), (-1, 1)] {
        lines.push(format!(
            "v {}{} {}{} 0",
            if sx < 0 { "-" } else { "" },
            h,
            if sy < 0 { "-" } else { "" },
            h
        ));
    }
    lines.extend([
        "vt 0 0".to_string(),
        "vt 1 0".into(),
        "vt 1 1".into(),
        "vt 0 1".into(),
        format!("usemtl {material}"),
        "f 1/1 2/2 3/3".into(),
        "f 1/1 3/3 4/4".into(),
    ]);
    (text(lines), mtl(&[(material, texture)], false, true))
}

fn rope() -> (Vec<u8>, Vec<u8>) {
    let x = 19.0 * K;
    let z = -500.0 * K;
    let v = 500.0 * 0.003;
    let mut lines = vec!["mtllib trapezerope.mtl".to_string()];
    for sz in [0.0, z] {
        for sx in [-x, x] {
            lines.push(format!("v {} 0 {}", sx, sz));
        }
    }
    lines.extend(["vt 0 0", "vt 1 0"].iter().map(|s| s.to_string()));
    lines.push(format!("vt 0 {v}"));
    lines.push(format!("vt 1 {v}"));
    lines.extend(
        [
            "usemtl rope",
            "f 1/1 2/2 3/3",
            "f 2/2 4/4 3/3",
            "f 3/3 2/2 1/1",
            "f 3/3 4/4 2/2",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    (
        text(lines),
        mtl(&[("rope", "trapezerope.png")], false, false),
    )
}

/// Return all OBJ/MTL files produced by the native procedural figures stage.
pub fn generate(effect_arc: &[u8]) -> Result<Vec<(String, Vec<u8>)>, ArchiveError> {
    let names = rarc_find(effect_arc, "particlenames.bcsv")?;
    let jpc = rarc_find(effect_arc, "particles.jpc")?;
    let ids = particle_ids(&names)?
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    let sizes = particle_sizes(&jpc)?
        .into_iter()
        .map(|(id, w, h)| (id as i32, (w, h)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let half = |name: &str| -> Result<f64, ArchiveError> {
        let id = *ids.get(name.as_bytes()).ok_or(ArchiveError::NotFound)?;
        let (w, h) = *sizes.get(&id).ok_or(ArchiveError::NotFound)?;
        if w != h {
            return Err(ArchiveError::InvalidRarc);
        }
        Ok(w as f64 * JPA_UNIT * K)
    };

    let mut out = Vec::new();
    let base = "assets/figures/";
    let (obj, mat) = creeper_stalk();
    out.push((format!("{base}creeperstalk/creeperstalk.obj"), obj));
    out.push((format!("{base}creeperstalk/creeperstalk.mtl"), mat));
    let (obj, mat) = plant_stalk();
    out.push((format!("{base}plantstalk/plantstalk.obj"), obj.clone()));
    out.push((format!("{base}plantstalk/plantstalk.mtl"), mat.clone()));
    out.push((format!("{base}plantstalk/lod.obj"), lod_of(&obj)));
    out.push((format!("{base}plantstalk/lod.mtl"), mat));
    let (obj, lod, mat) = tail_chain();
    out.push((
        format!("{base}dinopackuntailchain/dinopackuntailchain.obj"),
        obj,
    ));
    out.push((
        format!("{base}dinopackuntailchain/dinopackuntailchain.mtl"),
        mat.clone(),
    ));
    out.push((format!("{base}dinopackuntailchain/lod.obj"), lod));
    out.push((format!("{base}dinopackuntailchain/lod.mtl"), mat));
    let (obj, mat) = billboard(
        "capturebubble",
        "CaptureLight",
        "capture_ring.png",
        half("MarioGCaptureLight00")?,
        Some(3),
    );
    out.push((format!("{base}capturebubble/capturebubble.obj"), obj));
    out.push((format!("{base}capturebubble/capturebubble.mtl"), mat));
    for (name, particle) in [
        ("kamellabeamfire", "KameckBeamFire00"),
        ("kamellabeamfireready", "KameckBeamFireReady00"),
        ("kamellabeamturtle", "KameckBeamTurtle00"),
        ("kamellabeamturtleready", "KameckBeamTurtleReady00"),
    ] {
        let (obj, mat) = billboard(
            name,
            "NativeParticle",
            &format!("{name}.png"),
            half(particle)?,
            None,
        );
        out.push((format!("{base}{name}/{name}.obj"), obj));
        out.push((format!("{base}{name}/{name}.mtl"), mat));
    }
    let (obj, mat) = rope();
    out.push((format!("{base}trapezerope/trapezerope.obj"), obj));
    out.push((format!("{base}trapezerope/trapezerope.mtl"), mat));
    Ok(out)
}
