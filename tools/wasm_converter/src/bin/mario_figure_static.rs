use smg_iso_verifier::mario_figure;
use std::{env, fs, path::Path};

fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: mario_figure_static <Mario.arc> <output-root>");
        std::process::exit(2);
    }
    let archive = fs::read(&args[1]).expect("read Mario.arc");
    let files = mario_figure::generate_static(&archive).unwrap_or_else(|error| {
        eprintln!("could not generate static Mario figure assets: {error:?}");
        std::process::exit(1);
    });
    for (name, bytes) in &files {
        let relative = name.strip_prefix("assets/").expect("asset path prefix");
        let path = Path::new(&args[2]).join(relative);
        fs::create_dir_all(path.parent().unwrap()).expect("create output directory");
        fs::write(path, bytes).expect("write generated asset");
    }
    println!("generated {} static figure files", files.len());
}
