use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-env-changed=SMG_EXTRACT_BUFFER_MIB");
    println!("cargo:rerun-if-env-changed=SMG_ASSETC_LINKED_LIB_DIR");
    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = crate_dir.parent().unwrap().parent().unwrap();
    let mut files = BTreeMap::<String, [u8; 32]>::new();
    let source_dir = root.join("asset_construction");

    for entry in fs::read_dir(&source_dir).expect("read asset_construction") {
        let path = entry
            .expect("source directory entry")
            .path()
            .join("sources.txt");
        if !path.is_file() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let text = fs::read_to_string(&path).expect("read source manifest");
        for (line_number, line) in text.lines().enumerate() {
            let mut fields = line.split_whitespace();
            let Some(hash) = fields.next() else { continue };
            let source = fields.collect::<Vec<_>>().join(" ");
            if source.is_empty() {
                panic!(
                    "{}:{}: missing source path",
                    path.display(),
                    line_number + 1
                );
            }
            let decoded = decode_sha256(hash).unwrap_or_else(|| {
                panic!("{}:{}: malformed SHA-256", path.display(), line_number + 1)
            });
            if let Some(old) = files.insert(source.clone(), decoded) {
                assert_eq!(old, decoded, "conflicting hashes for {source}");
            }
        }
    }

    let mut generated = String::from("static SOURCE_CATALOG: &[Source] = &[\n");
    for (path, sha256) in files {
        let bytes = sha256
            .iter()
            .map(|b| format!("0x{b:02x}"))
            .collect::<Vec<_>>()
            .join(", ");
        generated.push_str(&format!(
            "    Source {{ path: {:?}, sha256: [{bytes}], dol: {} }},\n",
            path,
            path.starts_with("../")
        ));
    }
    generated.push_str("];\n");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out_dir.join("source_catalog.rs"), generated)
        .expect("write generated source catalog");
    let asset_config = root.join("assets/assets.cfg");
    println!("cargo:rerun-if-changed={}", asset_config.display());
    fs::copy(&asset_config, out_dir.join("asset_config.txt"))
        .expect("embed native asset compiler configuration");
    let asset_palette = root.join("build/constructed/assets/palette.rgba");
    println!("cargo:rerun-if-changed={}", asset_palette.display());
    fs::copy(&asset_palette, out_dir.join("asset_palette.rgba"))
        .expect("embed native asset compiler palette");
    let range_mib = env::var("SMG_EXTRACT_BUFFER_MIB").unwrap_or_else(|_| "8".into());
    assert!(matches!(range_mib.as_str(), "8" | "64" | "128" | "256"));
    fs::write(
        out_dir.join("range_config.rs"),
        format!("pub const EXTRACT_BUFFER_MIB: u32 = {range_mib};\n"),
    )
    .expect("write range configuration");
    embed_recipe_packs(&source_dir, &out_dir);
    let aa_table = source_dir.join("textures/bake_aa64.bin");
    println!("cargo:rerun-if-changed={}", aa_table.display());
    fs::copy(&aa_table, out_dir.join("bake_aa64.bin")).expect("embed bake AA64 table");

    if env::var_os("CARGO_FEATURE_PROCESSOR").is_some()
        && env::var("TARGET").is_ok_and(|target| target.starts_with("wasm32-"))
    {
        let object = PathBuf::from(
            env::var_os("SMG_BAKE_CORE_OBJECT")
                .expect("SMG_BAKE_CORE_OBJECT must point to the compiled texture bake core"),
        );
        assert!(
            object.is_file(),
            "missing texture bake core object: {}",
            object.display()
        );
        println!("cargo:rerun-if-changed={}", object.display());
        println!(
            "cargo:rerun-if-changed={}",
            root.join("tools/asset_construction/textures_bake_core.c")
                .display()
        );
        println!("cargo:rustc-link-arg={}", object.display());
    }

    if env::var_os("CARGO_FEATURE_ASSETC_LINKED").is_some() {
        assert!(
            env::var("TARGET").is_ok_and(|target| target.starts_with("wasm32-")),
            "the linked C asset compiler requires a wasm32 target"
        );
        let lib_dir = PathBuf::from(
            env::var_os("SMG_ASSETC_LINKED_LIB_DIR")
                .expect("SMG_ASSETC_LINKED_LIB_DIR must point to the linked WASI C libraries"),
        );
        for library in ["libassetc.a", "libc.a", "libm.a"] {
            let path = lib_dir.join(library);
            assert!(path.is_file(), "missing linked asset compiler library: {}", path.display());
            println!("cargo:rerun-if-changed={}", path.display());
        }
        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        println!("cargo:rustc-link-lib=static=assetc");
        println!("cargo:rustc-link-lib=static=c");
        println!("cargo:rustc-link-lib=static=m");
        println!("cargo:rustc-link-arg=--allow-multiple-definition");
        println!("cargo:rerun-if-changed={}", root.join("tools/asset_compiler").display());
        println!("cargo:rerun-if-changed={}", root.join("tools/asset_compiler/wasm_alloc.c").display());
        println!("cargo:rerun-if-changed={}", root.join("tools/asset_compiler/wasm_vfs.c").display());
    }
}

fn embed_recipe_packs(source_dir: &Path, out_dir: &Path) {
    let mut paths = Vec::new();
    find_paks(source_dir, &mut paths);
    paths.retain(|path| {
        let rel = path.strip_prefix(source_dir).unwrap().to_string_lossy();
        [
            "audio/",
            "figures/",
            "mario_figure/",
            "stages/",
            "textures/",
            "ui_headers/",
        ]
        .iter()
        .any(|prefix| rel.starts_with(prefix))
    });
    paths.sort();

    let mut source = String::from("pub static EMBEDDED_PACKS: &[EmbeddedPack] = &[\n");
    for (index, path) in paths.iter().enumerate() {
        println!("cargo:rerun-if-changed={}", path.display());
        let archive = Command::new("xz")
            .args(["--decompress", "--stdout"])
            .arg(path)
            .output()
            .unwrap_or_else(|e| panic!("run xz to unpack {}: {e}", path.display()));
        assert!(archive.status.success(), "xz failed on {}", path.display());
        let raw = archive.stdout;
        let index_len =
            u32::from_le_bytes(raw.get(..4).expect("pak header").try_into().unwrap()) as usize;
        let index_end = 4usize.checked_add(index_len).expect("pak index overflow");
        let entries: Vec<(String, usize, usize)> =
            serde_json::from_slice(raw.get(4..index_end).expect("pak index bytes"))
                .unwrap_or_else(|e| panic!("parse pak index in {}: {e}", path.display()));
        let payload = raw.get(index_end..).expect("pak payload");
        let payload_path = out_dir.join(format!("recipe_pack_{index:02}.bin"));
        fs::write(&payload_path, payload).expect("write unpacked recipe pack");

        let rel = path.strip_prefix(source_dir).unwrap().to_string_lossy();
        source.push_str(&format!(
            "    EmbeddedPack {{ name: {:?}, data: include_bytes!({:?}), entries: &[\n",
            rel.replace('\\', "/"),
            payload_path,
        ));
        for (name, offset, size) in entries {
            let end = offset.checked_add(size).expect("pak entry overflow");
            assert!(
                end <= payload.len(),
                "pak entry outside {}: {name}",
                path.display()
            );
            source.push_str(&format!("        ({:?}, {offset}, {size}),\n", name));
        }
        source.push_str("    ] },\n");
    }
    source.push_str("];\n");
    fs::write(out_dir.join("embedded_packs.rs"), source).expect("write embedded recipe pack index");
}

fn find_paks(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            find_paks(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "pak") {
            out.push(path);
        }
    }
}

fn decode_sha256(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut bytes = [0; 32];
    for (i, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let digit = |b: u8| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        };
        bytes[i] = digit(pair[0])?
            .checked_mul(16)?
            .checked_add(digit(pair[1])?)?;
    }
    Some(bytes)
}
