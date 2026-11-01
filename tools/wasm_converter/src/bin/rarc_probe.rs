use sha2::{Digest, Sha256};
use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: rarc_probe ARCHIVE DIRECTORY".into());
    }
    let archive = fs::read(&args[1])?;
    let directory = args[2].to_string_lossy();
    let files = smg_iso_verifier::asset_formats::rarc_listdir(&archive, &directory)
        .map_err(|e| format!("RARC read failed: {e:?}"))?;
    for (name, bytes) in files {
        println!("{}\t{}\t{:x}", name, bytes.len(), Sha256::digest(&bytes));
    }
    Ok(())
}
