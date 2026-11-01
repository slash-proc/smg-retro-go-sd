use smg_iso_verifier::textures_gx::SimpleImageGenerator;
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let source_root = PathBuf::from(
        args.next()
            .ok_or("usage: textures_probe SOURCE_ROOT OUTPUT_ROOT")?,
    );
    let output_root = PathBuf::from(args.next().ok_or("missing output root")?);
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let generator = SimpleImageGenerator::new().map_err(|error| format!("{error:?}"))?;
    let archives = generator
        .source_archives()
        .map_err(|error| format!("{error:?}"))?;
    let mut outputs = 0usize;
    for archive_name in archives {
        let archive_path = source_root.join(&archive_name);
        let archive = fs::read(&archive_path)
            .map_err(|error| format!("{}: {error}", archive_path.display()))?;
        for (paths, png) in generator
            .generate_archive(&archive_name, &archive)
            .map_err(|error| format!("{archive_name}: {error:?}"))?
        {
            for path in paths {
                let output = output_root.join(&path);
                if let Some(parent) = output.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                fs::write(&output, &png).map_err(|error| error.to_string())?;
                outputs += 1;
            }
        }
    }
    println!(
        "generated {outputs} GX-backed image paths from {} archives",
        generator
            .source_archives()
            .map_err(|error| format!("{error:?}"))?
            .len()
    );
    Ok(())
}
