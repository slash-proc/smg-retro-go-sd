use smg_iso_verifier::generic_figures;
use std::{collections::BTreeMap, env, fs, path::Path};

fn main() {
    let mut args = env::args().skip(1);
    let source_root = args.next().expect("source root");
    let output_root = args.next().expect("output directory");
    let mut archives = BTreeMap::new();
    for path in generic_figures::source_archives().expect("figure source list") {
        let data = fs::read(Path::new(&source_root).join(&path))
            .unwrap_or_else(|error| panic!("read {path}: {error}"));
        archives.insert(path, data);
    }
    let files = generic_figures::generate(&archives)
        .unwrap_or_else(|error| panic!("generate generic figures: {error:?}"));
    for (path, data) in &files {
        let output = Path::new(&output_root).join(path);
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(output, data).unwrap();
    }
    println!("generated {} generic figure files", files.len());
}
