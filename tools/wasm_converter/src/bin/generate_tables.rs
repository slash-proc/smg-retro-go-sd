use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(
        env::args_os()
            .nth(1)
            .ok_or("usage: generate_tables OUTPUT_DIR")?,
    );
    fs::create_dir_all(&out)?;
    for (name, bytes) in
        smg_iso_verifier::lookup_tables::named(smg_iso_verifier::lookup_tables::build())
    {
        fs::write(out.join(format!("{name}.bin")), bytes)?;
    }
    Ok(())
}
