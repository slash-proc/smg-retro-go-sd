use serde_json::{json, Value};
use smg_iso_verifier::{
    audio_jaudio::{decode_wave, mix_to_pcm, render_note, AudioArchive},
    audio_seq::simulate,
};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let archive_path = args
        .next()
        .ok_or("usage: audio_sequence_probe SMR.SZS CASES.json")?;
    let cases_path = args
        .next()
        .ok_or("usage: audio_sequence_probe SMR.SZS CASES.json")?;
    let jai_sequence_path = args.next();
    let waves_root = args.next().map(PathBuf::from);
    if args.next().is_some() {
        return Err("usage: audio_sequence_probe SMR.SZS CASES.json".into());
    }
    let archive = AudioArchive::parse(&fs::read(archive_path)?)?;
    let recipe_text = smg_iso_verifier::asset_packs::get("audio/audio.pak", "recipes.json")
        .ok_or("audio recipes are missing from embedded packs")?;
    let recipes: Value = serde_json::from_slice(recipe_text)?;
    let pow2 = recipes["pow2"]
        .as_object()
        .ok_or("audio recipe power table is missing")?
        .iter()
        .map(|(exponent, value)| {
            let exponent =
                smg_iso_verifier::audio_assets::parse_hex_float(&Value::String(exponent.clone()))?;
            let value = smg_iso_verifier::audio_assets::parse_hex_float(value)?;
            Ok((exponent.to_bits(), value))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let jai_sequence = jai_sequence_path.map(fs::read).transpose()?;
    let cases: Vec<Value> = serde_json::from_slice(&fs::read(cases_path)?)?;
    let mut outputs = Vec::with_capacity(cases.len());
    for case in cases {
        let group = case["group"].as_u64().ok_or("case missing group")? as usize;
        let index = case["index"].as_u64().ok_or("case missing index")? as usize;
        let sequence_data = if case["kind"].as_str() == Some("bgm") {
            archive.bgm_sequence(
                case["sid"].as_u64().ok_or("BGM case missing sid")? as u32,
                jai_sequence
                    .as_deref()
                    .ok_or("BGM case requires JaiSeq.arc")?,
            )?
        } else {
            Vec::new()
        };
        let (data, start) = if case["kind"].as_str() == Some("bgm") {
            (&sequence_data[..], 0)
        } else {
            let start = *archive
                .sequence_offsets
                .get(group)
                .and_then(|sequences| sequences.get(index))
                .ok_or("sequence index out of range")?;
            (&archive.bsc[..], start)
        };
        let ports = case["ports"].as_object().map(|ports| {
            ports
                .iter()
                .filter_map(|(key, value)| Some((key.parse().ok()?, value.as_u64()? as u16)))
                .collect::<BTreeMap<u8, u16>>()
        });
        let notes = simulate(
            data,
            start,
            ports.as_ref(),
            case["seed"].as_u64(),
            case["max_time"].as_f64().unwrap_or(6.0),
            case["max_steps"].as_u64().unwrap_or(20_000) as usize,
        );
        let resolved: Vec<_> = notes
            .iter()
            .filter_map(|note| archive.resolve_note_with_pow2(note, &pow2))
            .collect();
        outputs.push(json!({
            "name": case["name"],
            "resolved": resolved.len(),
            "samples": resolved.iter().map(|resolved| json!({
                "wave_archive": resolved.wave.archive,
                "wave_id": resolved.wave.id,
                "wave_rate": resolved.wave.rate,
                "ratio": resolved.ratio,
                "volume": resolved.volume,
                "direct_release": resolved.direct_release,
                "has_oscillator": resolved.oscillator.is_some(),
                "render": waves_root.as_ref().and_then(|root| {
                    let bytes = fs::read(root.join(&resolved.wave.archive)).ok()?;
                    let pcm = decode_wave(&bytes, &resolved.wave).ok()?;
                    let rendered = render_note(
                        resolved,
                        &pcm,
                        case["rate"].as_u64().unwrap_or(resolved.wave.rate as u64) as usize,
                        case["render_seconds"].as_f64().unwrap_or(0.25),
                        case["flavor"].as_u64().unwrap_or(0) as u8,
                        case["adsr"].as_bool().unwrap_or(false),
                    )?;
                    let peak = rendered.samples.iter().fold(0.0f64, |peak, sample| peak.max(sample.abs()));
                    let hash = rendered
                        .samples
                        .iter()
                        .flat_map(|sample| sample.to_le_bytes())
                        .fold(0xcbf29ce484222325u64, |hash, byte| {
                            (hash ^ byte as u64).wrapping_mul(0x100000001b3)
                        });
                    let chunk_hashes = rendered
                        .samples
                        .chunks(64)
                        .map(|samples| {
                            let hash = samples
                                .iter()
                                .flat_map(|sample| sample.to_le_bytes())
                                .fold(0xcbf29ce484222325u64, |hash, byte| {
                                    (hash ^ byte as u64).wrapping_mul(0x100000001b3)
                                });
                            format!("{hash:016x}")
                        })
                        .collect::<Vec<_>>();
                    let rate = case["rate"].as_u64().unwrap_or(resolved.wave.rate as u64) as usize;
                    let max_samples = (case["render_seconds"].as_f64().unwrap_or(0.25)
                        * rate as f64) as usize;
                    let copies = case["copies"].as_u64().unwrap_or(1).max(1) as usize;
                    let parts = vec![rendered.clone(); copies];
                    let pcm = mix_to_pcm(
                        &parts,
                        max_samples,
                        case["trim"].as_bool().unwrap_or(false),
                    );
                    let pcm_hash = pcm.iter().flat_map(|sample| sample.to_le_bytes()).fold(
                        0xcbf29ce484222325u64,
                        |hash, byte| (hash ^ byte as u64).wrapping_mul(0x100000001b3),
                    );
                    let release_window = resolved
                        .note
                        .gate
                        .map(|gate| (gate * rate as f64).round_ties_even() as usize)
                        .unwrap_or(0);
                    let release_samples = &rendered.samples[
                        release_window.saturating_sub(4)
                            ..(release_window + 12).min(rendered.samples.len())
                    ];
                    let envelope = smg_iso_verifier::audio_jaudio::note_envelope(
                        resolved.oscillator.as_ref(),
                        resolved.note.gate,
                        case["render_seconds"].as_f64().unwrap_or(0.25),
                        resolved.direct_release,
                    );
                    Some(json!({
                        "offset": rendered.offset,
                        "length": rendered.samples.len(),
                        "peak": peak,
                        "fnv1a64_f64le": format!("{hash:016x}"),
                        "chunk_hashes": chunk_hashes,
                        "first_samples": rendered.samples.iter().take(8).collect::<Vec<_>>(),
                        "last_samples": rendered.samples.iter().rev().take(12).copied().collect::<Vec<_>>(),
                        "release_index": release_window,
                        "release_samples": release_samples,
                        "envelope_count": envelope.len(),
                        "envelope_tail": envelope.iter().rev().take(12).copied().collect::<Vec<_>>(),
                        "pcm16_length": pcm.len(),
                        "pcm16_fnv1a64_le": format!("{pcm_hash:016x}"),
                    }))
                }),
            })).collect::<Vec<_>>(),
            "notes": notes.iter().map(|note| json!({
                "time": note.time,
                "bank": note.bank,
                "program": note.program,
                "key": note.key,
                "velocity": note.velocity,
                "gate": note.gate,
                "volume": note.volume,
                "pitch_octaves": note.pitch_octaves,
                "adsr": note.adsr,
            })).collect::<Vec<_>>(),
        }));
    }
    println!("{}", serde_json::to_string(&outputs)?);
    Ok(())
}
