use smg_iso_verifier::audio_jaudio::{decode_wave, AudioArchive};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let path = args
        .next()
        .ok_or("usage: audio_archive_probe SMR.SZS WAVES_ROOT")?;
    let waves_root = PathBuf::from(
        args.next()
            .ok_or("usage: audio_archive_probe SMR.SZS WAVES_ROOT")?,
    );
    if args.next().is_some() {
        return Err("usage: audio_archive_probe SMR.SZS WAVES_ROOT".into());
    }
    let bytes = fs::read(path)?;
    let archive = AudioArchive::parse(&bytes)?;
    println!(
        "{} wave systems, {} waves, {} banks, {} BSC sequence groups, {} BST bytes, {} BSC bytes",
        archive.waves.len(),
        archive.wave_count(),
        archive.banks.len(),
        archive.sequence_offsets.len(),
        archive.bst.len(),
        archive.bsc.len()
    );
    let mut wave_archives = BTreeMap::<String, Vec<u8>>::new();
    let mut wave_count = 0usize;
    let mut missing_archives = BTreeMap::<String, ()>::new();
    let mut sample_count = 0usize;
    let mut checksum = 0xcbf29ce484222325u64;
    for waves in archive.waves.values() {
        for wave in waves.values() {
            let wave_path = waves_root.join(&wave.archive);
            if !wave_path.is_file() {
                missing_archives.insert(wave.archive.clone(), ());
                continue;
            }
            let data = if let Some(data) = wave_archives.get(&wave.archive) {
                data
            } else {
                let data = fs::read(&wave_path)?;
                wave_archives.insert(wave.archive.clone(), data);
                wave_archives
                    .get(&wave.archive)
                    .expect("inserted wave archive")
            };
            let pcm = decode_wave(data, wave)?;
            for sample in &pcm {
                for byte in sample.to_le_bytes() {
                    checksum = (checksum ^ byte as u64).wrapping_mul(0x100000001b3);
                }
            }
            wave_count += 1;
            sample_count += pcm.len();
        }
    }
    println!(
        "decoded {wave_count} available waves / {sample_count} samples; skipped {} absent archives; FNV-1a64 {checksum:016x}",
        missing_archives.len()
    );
    println!(
        "{} bank instruments across {} parsed banks",
        archive
            .banks
            .values()
            .map(|bank| bank.instruments.len())
            .sum::<usize>(),
        archive.banks.len()
    );
    for (group, sequences) in archive.sequence_offsets.iter().enumerate() {
        println!("sequence group {group}: {} entries", sequences.len());
    }
    Ok(())
}
