use smg_iso_verifier::audio_assets::AudioAstConverter;
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let source_dir = PathBuf::from(
        args.next()
            .ok_or("usage: audio_ast_probe SOURCE_ROOT GENERATED_AUDIO")?,
    );
    let generated_dir = PathBuf::from(
        args.next()
            .ok_or("usage: audio_ast_probe SOURCE_ROOT GENERATED_AUDIO")?,
    );
    let actual_dir = args.next().map(PathBuf::from);
    if args.next().is_some() {
        return Err("usage: audio_ast_probe SOURCE_ROOT GENERATED_AUDIO".into());
    }
    let mut converter = AudioAstConverter::new()?;
    let mut sources = converter.source_paths();
    sources.sort_by_key(|source| {
        if source == "AudioRes/SMR.szs"
            || source == "AudioRes/Seqs/JaiSeq.arc"
            || source.starts_with("AudioRes/Waves/")
        {
            0
        } else {
            1
        }
    });
    let mut checked = 0usize;
    for source in sources {
        println!("processing {source}");
        let archive = fs::read(source_dir.join(&source))?;
        for (asset, bytes) in converter.process_archive(&source, archive)? {
            let name = asset
                .strip_prefix("assets/audio/")
                .ok_or("invalid generated audio path")?;
            let expected = fs::read(generated_dir.join(name))?;
            if bytes != expected {
                let mismatch = bytes
                    .iter()
                    .zip(expected.iter())
                    .position(|(actual, expected)| actual != expected)
                    .unwrap_or(bytes.len().min(expected.len()));
                if let Some(actual_dir) = &actual_dir {
                    fs::create_dir_all(actual_dir)?;
                    fs::write(actual_dir.join(name), &bytes)?;
                }
                return Err(format!(
                    "{name}: generated {} bytes, expected {} bytes; first mismatch at {mismatch}; actual {:?}, expected {:?}",
                    bytes.len(),
                    expected.len(),
                    bytes.get(mismatch.saturating_sub(8)..(mismatch + 8).min(bytes.len())),
                    expected.get(mismatch.saturating_sub(8)..(mismatch + 8).min(expected.len())),
                )
                .into());
            }
            checked += 1;
            println!("matched {asset} ({} bytes)", bytes.len());
        }
    }
    println!("matched {checked} generated audio assets byte-for-byte");
    Ok(())
}
