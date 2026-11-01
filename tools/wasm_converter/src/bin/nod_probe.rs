use std::{
    collections::BTreeSet,
    env,
    error::Error,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use nod::{
    common::PartitionKind,
    read::{DiscOptions, DiscReader, PartitionOptions},
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("converter crate lives two directories below the project root")
        .to_path_buf()
}

fn source_paths(root: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut paths = BTreeSet::new();
    for entry in fs::read_dir(root.join("asset_construction"))? {
        let entry = entry?;
        let manifest = entry.path().join("sources.txt");
        if !manifest.is_file() {
            continue;
        }
        for line in fs::read_to_string(manifest)?
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let (_, path) = line
                .split_once(char::is_whitespace)
                .ok_or_else(|| format!("malformed source row: {line}"))?;
            paths.insert(path.trim().to_string());
        }
    }
    Ok(paths)
}

fn disc_path(path: &str) -> String {
    let path = path.strip_prefix("../").unwrap_or(path);
    format!("/{path}")
}

fn oracle_path(root: &Path, path: &str) -> PathBuf {
    if let Some(path) = path.strip_prefix("../") {
        root.join("build/orig").join(path)
    } else {
        root.join("build/orig/files").join(path)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let iso = args.next().ok_or("usage: nod_probe ISO")?;
    if args.next().is_some() {
        return Err("usage: nod_probe ISO".into());
    }
    let root = repo_root();
    let paths = source_paths(&root)?;

    let disc = DiscReader::new(iso, &DiscOptions::default())?;
    if disc.header().game_id_str() != "RMGE01" {
        return Err(format!("unexpected disc ID: {}", disc.header().game_id_str()).into());
    }
    let mut partition = disc.open_partition_kind(
        PartitionKind::Data,
        &PartitionOptions {
            validate_hashes: false,
        },
    )?;
    let meta = partition.meta()?;
    let fst = meta.fst()?;

    let mut total_bytes = 0u64;
    let mut missing = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if path == "../sys/main.dol" {
            let oracle = fs::read(oracle_path(&root, path))?;
            if meta.raw_dol.as_ref() != oracle.as_slice() {
                return Err(format!("byte mismatch for {path}").into());
            }
            total_bytes += meta.raw_dol.len() as u64;
            continue;
        }
        let node = match fst.find(&disc_path(path)) {
            Some((_, node)) => node,
            None => {
                missing.push(path.clone());
                continue;
            }
        };
        let mut reader = partition.open_file(node)?;
        let mut extracted = Vec::new();
        reader.read_to_end(&mut extracted)?;
        let oracle = fs::read(oracle_path(&root, path))?;
        if extracted != oracle {
            return Err(format!("byte mismatch for {path}").into());
        }
        total_bytes += extracted.len() as u64;
        if (index + 1) % 50 == 0 {
            println!(
                "verified {}/{} selected files ({total_bytes} bytes)",
                index + 1,
                paths.len()
            );
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "{} selected FST paths missing; first: {}",
            missing.len(),
            missing[0]
        )
        .into());
    }
    println!(
        "nod verified {} selected files, {total_bytes} bytes, byte-for-byte against build/orig",
        paths.len()
    );
    Ok(())
}
