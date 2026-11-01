//! Range-fed nod extraction probe. The host supplies random ISO ranges through
//! the same export-only request/supply ABI as the verifier.

use std::{
    cmp,
    io::{self, Read},
    sync::Arc,
};

use nod::{
    common::PartitionKind,
    read::{DiscOptions, DiscReader, DiscStream, PartitionMeta, PartitionOptions, PartitionReader},
};
use sha2::{Digest, Sha256};

const META_CACHE_LIMIT: usize = 16 * 1024 * 1024;
const RANGE_ALIGNMENT: u64 = 64 * 1024 * 1024;
const STATUS_MORE: u32 = 1;
const STATUS_NEED_INPUT: u32 = 2;
const STATUS_ERROR: u32 = 3;

struct Source {
    path: &'static str,
    sha256: [u8; 32],
    dol: bool,
}

include!(concat!(env!("OUT_DIR"), "/source_catalog.rs"));

#[derive(Clone)]
struct CachedRange {
    offset: u64,
    data: Vec<u8>,
}

struct PendingStage {
    recipe: crate::stage_recipe::StageRecipe,
    stage: crate::stage_galaxy::StageGalaxy,
}

static mut META_CACHE: Vec<CachedRange> = Vec::new();
static mut FILE_CACHE: Vec<CachedRange> = Vec::new();
static mut META_CACHE_BYTES: usize = 0;
static mut FILE_CACHE_BYTES: usize = 0;
static mut FILE_ORDER: Vec<usize> = Vec::new();
static mut NEXT_FILE: usize = 0;
static mut DISC: Option<DiscReader> = None;
static mut PARTITION: Option<Box<dyn PartitionReader>> = None;
static mut PARTITION_META: Option<PartitionMeta> = None;
static mut MAIN_DOL: Option<Arc<[u8]>> = None;
static mut FILE_REMAINING: u64 = 0;
static mut FILE_STARTED: bool = false;
static mut FILE_HASH: Option<Sha256> = None;
static mut CAPTURING_EFFECT: bool = false;
static mut EFFECT_ARC: Vec<u8> = Vec::new();
static mut CAPTURING_MARIO: bool = false;
static mut MARIO_ARC: Vec<u8> = Vec::new();
static mut CAPTURING_MARIO_ANIME: bool = false;
static mut MARIO_ANIME_ARC: Vec<u8> = Vec::new();
static mut CAPTURING_BEE_WING: bool = false;
static mut BEE_WING_ARC: Vec<u8> = Vec::new();
static mut WANTED_FIGURE_ARCS: Vec<String> = Vec::new();
static mut FIGURE_ARCS: Vec<(String, Vec<u8>)> = Vec::new();
static mut CURRENT_FIGURE_ARC: Vec<u8> = Vec::new();
static mut CAPTURING_FIGURE_ARC: bool = false;
static mut FIGURE_ARC_PATH: String = String::new();
static mut WANTED_TEXTURE_ARCS: Vec<String> = Vec::new();
static mut TEXTURE_GENERATOR: Option<crate::textures_gx::SimpleImageGenerator> = None;
static mut PROCESS_TEXTURE_ARC: bool = false;
static mut CURRENT_TEXTURE_ARC: Vec<u8> = Vec::new();
static mut CAPTURING_TEXTURE_ARC: bool = false;
static mut WANTED_STAGE_ARCS: Vec<String> = Vec::new();
static mut CAPTURING_STAGE_ARC: bool = false;
static mut STAGE_ARC_PATH: String = String::new();
static mut CURRENT_STAGE_ARC: Vec<u8> = Vec::new();
static mut STAGE_ARCHIVES: std::collections::BTreeMap<String, Vec<u8>> =
    std::collections::BTreeMap::new();
static mut STAGE_SOURCE_REFS: std::collections::BTreeMap<String, usize> =
    std::collections::BTreeMap::new();
static mut PENDING_STAGES: Vec<PendingStage> = Vec::new();
static mut CAPTURING_STAGE_DATA: bool = false;
static mut STAGE_DATA_PATH: String = String::new();
static mut CURRENT_STAGE_DATA_ARC: Vec<u8> = Vec::new();
static mut STAGE_DATA_ARCS: std::collections::BTreeMap<String, Vec<u8>> =
    std::collections::BTreeMap::new();
static mut STAGE_DATA_DONE: usize = 0;
static mut STAGE_DATA_EXPECTED: usize = 0;
static mut STAGES_INITIALIZED: bool = false;
static mut GENERATED_STAGE_ASSETS: Vec<(String, Vec<u8>)> = Vec::new();
static mut GENERATED_ASSETS: Vec<(String, Vec<u8>)> = Vec::new();
static mut GENERATED_TEXTURES: Vec<(Vec<String>, Vec<u8>)> = Vec::new();
static mut WANTED_AUDIO_SOURCES: Vec<String> = Vec::new();
static mut AUDIO_GENERATOR: Option<crate::audio_assets::AudioAstConverter> = None;
static mut CAPTURING_AUDIO: bool = false;
static mut CURRENT_AUDIO: Vec<u8> = Vec::new();
static mut IMAGE_SIZE: u64 = 0;
static mut PENDING: bool = false;
static mut PENDING_OFFSET: u64 = 0;
static mut PENDING_LEN: u32 = 0;
static mut PENDING_META: bool = true;
static mut META_READY: bool = false;
static mut STARTED: bool = false;
static mut FINISHED: bool = false;
static mut ERROR_CODE: u32 = 0;
static mut DEBUG_STAGE: u32 = 0;
static mut FILES_DONE: u32 = 0;
static mut BYTES_DONE: u64 = 0;

/// Starts a range-fed selective extraction proof. `buffer_mib` is the maximum
/// host request size; the module caches disc metadata and two file windows.
pub(crate) extern "C" fn extract_begin(size_lo: u32, size_hi: u32, buffer_mib: u32) -> u32 {
    let size = ((size_hi as u64) << 32) | size_lo as u64;
    if size != 4_699_979_776 || !matches!(buffer_mib, 8 | 64 | 128 | 256) {
        return STATUS_ERROR;
    }
    let mut wanted_figure_arcs = match crate::generic_figures::source_archives() {
        Ok(paths) => paths,
        Err(_) => return STATUS_ERROR,
    };
    let mut wanted_stage_arcs = match crate::stage_recipe::all_stage_source_archives() {
        Ok(paths) => paths,
        Err(_) => return STATUS_ERROR,
    };
    let texture_generator = match crate::textures_gx::SimpleImageGenerator::new() {
        Ok(generator) => generator,
        Err(_) => return STATUS_ERROR,
    };
    let audio_generator = match crate::audio_assets::AudioAstConverter::new() {
        Ok(generator) => generator,
        Err(_) => return STATUS_ERROR,
    };
    let wanted_audio_sources = audio_generator.source_paths();
    let wanted_texture_arcs = match texture_generator.source_archives() {
        Ok(paths) => paths,
        Err(_) => return STATUS_ERROR,
    };
    match texture_generator.geometry_source_archives() {
        Ok(paths) => wanted_figure_arcs.extend(paths),
        Err(_) => return STATUS_ERROR,
    }
    wanted_figure_arcs.sort();
    wanted_figure_arcs.dedup();
    wanted_stage_arcs.sort();
    wanted_stage_arcs.dedup();
    let stage_data_expected = SOURCE_CATALOG
        .iter()
        .filter(|source| source.path.starts_with("StageData/"))
        .count();
    let capacity = buffer_mib as usize * 1024 * 1024;
    unsafe {
        crate::INPUT.clear();
        if crate::INPUT.try_reserve_exact(capacity).is_err() {
            return 4;
        }
        crate::INPUT.resize(capacity, 0);
        crate::INPUT_CAPACITY = capacity;
        crate::FILE_SIZE = size;
        crate::OFFSET = 0;
        crate::INPUT_LEN = 0;
        crate::SUPPLIED = false;
        crate::STARTED = true;
        crate::DONE = false;
        META_CACHE.clear();
        FILE_CACHE.clear();
        META_CACHE_BYTES = 0;
        FILE_CACHE_BYTES = 0;
        FILE_ORDER.clear();
        NEXT_FILE = 0;
        DISC = None;
        PARTITION = None;
        PARTITION_META = None;
        MAIN_DOL = None;
        FILE_REMAINING = 0;
        FILE_STARTED = false;
        FILE_HASH = None;
        CAPTURING_EFFECT = false;
        EFFECT_ARC.clear();
        CAPTURING_MARIO = false;
        MARIO_ARC.clear();
        CAPTURING_MARIO_ANIME = false;
        MARIO_ANIME_ARC.clear();
        CAPTURING_BEE_WING = false;
        BEE_WING_ARC.clear();
        WANTED_FIGURE_ARCS = wanted_figure_arcs;
        FIGURE_ARCS.clear();
        CURRENT_FIGURE_ARC.clear();
        CAPTURING_FIGURE_ARC = false;
        FIGURE_ARC_PATH.clear();
        WANTED_TEXTURE_ARCS = wanted_texture_arcs;
        TEXTURE_GENERATOR = Some(texture_generator);
        PROCESS_TEXTURE_ARC = false;
        CURRENT_TEXTURE_ARC.clear();
        CAPTURING_TEXTURE_ARC = false;
        WANTED_STAGE_ARCS = wanted_stage_arcs;
        CAPTURING_STAGE_ARC = false;
        STAGE_ARC_PATH.clear();
        CURRENT_STAGE_ARC.clear();
        STAGE_ARCHIVES.clear();
        STAGE_SOURCE_REFS.clear();
        PENDING_STAGES.clear();
        CAPTURING_STAGE_DATA = false;
        STAGE_DATA_PATH.clear();
        CURRENT_STAGE_DATA_ARC.clear();
        STAGE_DATA_ARCS.clear();
        STAGE_DATA_DONE = 0;
        STAGE_DATA_EXPECTED = stage_data_expected;
        STAGES_INITIALIZED = false;
        GENERATED_STAGE_ASSETS.clear();
        GENERATED_ASSETS.clear();
        GENERATED_TEXTURES.clear();
        WANTED_AUDIO_SOURCES = wanted_audio_sources;
        AUDIO_GENERATOR = Some(audio_generator);
        CAPTURING_AUDIO = false;
        CURRENT_AUDIO.clear();
        IMAGE_SIZE = size;
        PENDING = false;
        PENDING_OFFSET = 0;
        PENDING_LEN = 0;
        PENDING_META = true;
        META_READY = false;
        STARTED = true;
        FINISHED = false;
        ERROR_CODE = 0;
        DEBUG_STAGE = 0;
        FILES_DONE = 0;
        BYTES_DONE = 0;
    }
    0
}

/// Replays nod's short-lived read operation after each supplied range. Cached
/// ranges let the operation advance until it reaches the next missing window.
pub(crate) extern "C" fn extract_step() -> u32 {
    unsafe {
        if !STARTED || FINISHED {
            return STATUS_ERROR;
        }
        if crate::SUPPLIED {
            let len = crate::INPUT_LEN;
            if len == 0 || crate::OFFSET + len as u64 > IMAGE_SIZE {
                ERROR_CODE = 1;
                return STATUS_ERROR;
            }
            if PENDING_META {
                make_cache_room(
                    &mut META_CACHE,
                    &mut META_CACHE_BYTES,
                    len,
                    META_CACHE_LIMIT,
                );
                insert_range(
                    &mut META_CACHE,
                    &mut META_CACHE_BYTES,
                    CachedRange {
                        offset: crate::OFFSET,
                        data: crate::INPUT[..len].to_vec(),
                    },
                );
            } else {
                // The selected ISO corpus totals less than 256 MiB, so a
                // single 256 MiB cache window can cover every selected source
                // while leaving room for the archives retained for generation.
                let limit = if crate::INPUT_CAPACITY >= 256 * 1024 * 1024 {
                    crate::INPUT_CAPACITY
                } else {
                    crate::INPUT_CAPACITY.saturating_mul(2)
                };
                make_cache_room(&mut FILE_CACHE, &mut FILE_CACHE_BYTES, len, limit);
                insert_range(
                    &mut FILE_CACHE,
                    &mut FILE_CACHE_BYTES,
                    CachedRange {
                        offset: crate::OFFSET,
                        data: crate::INPUT[..len].to_vec(),
                    },
                );
            }
            crate::SUPPLIED = false;
            crate::INPUT_LEN = 0;
            PENDING = false;
        }

        match try_one_step() {
            Ok(Some((next_index, digest))) => {
                let source_index = FILE_ORDER[next_index];
                if digest != SOURCE_CATALOG[source_index].sha256 {
                    ERROR_CODE = 5;
                    return STATUS_ERROR;
                }
                if CAPTURING_AUDIO {
                    let path = SOURCE_CATALOG[source_index].path;
                    let generated = if let Some(generator) = AUDIO_GENERATOR.as_mut() {
                        match generator.process_archive(path, core::mem::take(&mut CURRENT_AUDIO)) {
                            Ok(files) => files,
                            Err(_) => {
                                ERROR_CODE = 9;
                                return STATUS_ERROR;
                            }
                        }
                    } else {
                        ERROR_CODE = 9;
                        return STATUS_ERROR;
                    };
                    GENERATED_ASSETS.extend(generated);
                    CAPTURING_AUDIO = false;
                }
                if CAPTURING_STAGE_DATA {
                    STAGE_DATA_ARCS.insert(
                        core::mem::take(&mut STAGE_DATA_PATH),
                        core::mem::take(&mut CURRENT_STAGE_DATA_ARC),
                    );
                    STAGE_DATA_DONE += 1;
                    CAPTURING_STAGE_DATA = false;
                }
                if PROCESS_TEXTURE_ARC {
                    let path = SOURCE_CATALOG[source_index].path.to_owned();
                    let generated = if let Some(generator) = TEXTURE_GENERATOR.as_ref() {
                        let archive = if CAPTURING_FIGURE_ARC {
                            &CURRENT_FIGURE_ARC
                        } else {
                            &CURRENT_TEXTURE_ARC
                        };
                        match generator.generate_archive(&path, archive) {
                            Ok(files) => files,
                            Err(_) => {
                                ERROR_CODE = 7;
                                return STATUS_ERROR;
                            }
                        }
                    } else {
                        ERROR_CODE = 7;
                        return STATUS_ERROR;
                    };
                    GENERATED_TEXTURES.extend(generated);
                    if !CAPTURING_FIGURE_ARC {
                        CURRENT_TEXTURE_ARC.clear();
                    }
                    CAPTURING_TEXTURE_ARC = false;
                    PROCESS_TEXTURE_ARC = false;
                }
                if CAPTURING_FIGURE_ARC {
                    FIGURE_ARCS.push((
                        FIGURE_ARC_PATH.clone(),
                        core::mem::take(&mut CURRENT_FIGURE_ARC),
                    ));
                    FIGURE_ARC_PATH.clear();
                    CAPTURING_FIGURE_ARC = false;
                }
                if CAPTURING_STAGE_ARC {
                    let path = core::mem::take(&mut STAGE_ARC_PATH);
                    STAGE_ARCHIVES.insert(path, core::mem::take(&mut CURRENT_STAGE_ARC));
                    CAPTURING_STAGE_ARC = false;
                }
                if !STAGES_INITIALIZED && STAGE_DATA_DONE == STAGE_DATA_EXPECTED {
                    if initialize_stages().is_err() {
                        ERROR_CODE = 8;
                        return STATUS_ERROR;
                    }
                }
                if STAGES_INITIALIZED && process_ready_stages().is_err() {
                    ERROR_CODE = 8;
                    return STATUS_ERROR;
                }
                FILES_DONE += 1;
                NEXT_FILE += 1;
                if NEXT_FILE == FILE_ORDER.len() {
                    if !STAGES_INITIALIZED || !PENDING_STAGES.is_empty() {
                        ERROR_CODE = 8;
                        return STATUS_ERROR;
                    }
                    if EFFECT_ARC.is_empty() {
                        ERROR_CODE = 6;
                        return STATUS_ERROR;
                    }
                    release_extraction_state();
                    TEXTURE_GENERATOR = None;
                    AUDIO_GENERATOR = None;
                    WANTED_AUDIO_SOURCES.clear();
                    let (generated_assets, geometry_textures) =
                        match crate::procedural_figures::generate(&EFFECT_ARC) {
                            Ok(mut files) => {
                                if MARIO_ARC.is_empty() {
                                    ERROR_CODE = 6;
                                    return STATUS_ERROR;
                                }
                                if MARIO_ANIME_ARC.is_empty() || BEE_WING_ARC.is_empty() {
                                    ERROR_CODE = 6;
                                    return STATUS_ERROR;
                                }
                                match crate::mario_figure::generate_all(
                                    &MARIO_ARC,
                                    &MARIO_ANIME_ARC,
                                    &BEE_WING_ARC,
                                ) {
                                    Ok(figure_files) => files.extend(figure_files),
                                    Err(_) => {
                                        ERROR_CODE = 6;
                                        return STATUS_ERROR;
                                    }
                                }
                                let mut archives: std::collections::BTreeMap<String, Vec<u8>> =
                                    core::mem::take(&mut FIGURE_ARCS).into_iter().collect();
                                archives.extend(
                                    STAGE_DATA_ARCS
                                        .iter()
                                        .map(|(path, bytes)| (path.clone(), bytes.clone())),
                                );
                                archives.insert(
                                    "ObjectData/Mario.arc".into(),
                                    core::mem::take(&mut MARIO_ARC),
                                );
                                let figure_files = match crate::generic_figures::generate(&archives)
                                {
                                    Ok(figure_files) => figure_files,
                                    Err(_) => {
                                        ERROR_CODE = 6;
                                        return STATUS_ERROR;
                                    }
                                };
                                files.extend(core::mem::take(&mut GENERATED_STAGE_ASSETS));
                                let geometry_textures =
                                    match crate::textures_gx::generate_geometry_bakes(
                                        &archives,
                                        &figure_files,
                                        &files,
                                        MAIN_DOL.as_deref().unwrap_or(&[]),
                                    ) {
                                        Ok(images) => images,
                                        Err(crate::textures_gx::TextureError::GeometryBake(
                                            diagnostic,
                                        )) => {
                                            ERROR_CODE = diagnostic;
                                            return STATUS_ERROR;
                                        }
                                        Err(_) => {
                                            ERROR_CODE = 7;
                                            return STATUS_ERROR;
                                        }
                                    };
                                STAGE_DATA_ARCS.clear();
                                STAGE_DATA_ARCS = std::collections::BTreeMap::new();
                                files.extend(figure_files);
                                files.extend(crate::procedural_images::generate());
                                files.extend(crate::fixed_asset_inputs::generate());
                                files.push((
                                    "assets.cfg".into(),
                                    crate::asset_packs::ASSET_CONFIG.to_vec(),
                                ));
                                files.push((
                                    "palette.rgba".into(),
                                    crate::asset_packs::ASSET_PALETTE.to_vec(),
                                ));
                                files.extend(
                                    crate::lookup_tables::named(crate::lookup_tables::build())
                                        .into_iter()
                                        .map(|(name, bytes)| (format!("assets/{name}.bin"), bytes)),
                                );
                                (files, geometry_textures)
                            }
                            Err(_) => {
                                ERROR_CODE = 6;
                                return STATUS_ERROR;
                            }
                        };
                    GENERATED_ASSETS.extend(generated_assets);
                    GENERATED_TEXTURES.extend(geometry_textures);
                    EFFECT_ARC.clear();
                    EFFECT_ARC.shrink_to_fit();
                    MARIO_ARC.clear();
                    MARIO_ARC.shrink_to_fit();
                    MARIO_ANIME_ARC.clear();
                    MARIO_ANIME_ARC.shrink_to_fit();
                    BEE_WING_ARC.clear();
                    BEE_WING_ARC.shrink_to_fit();
                    MAIN_DOL = None;
                    FIGURE_ARCS.clear();
                    FIGURE_ARCS.shrink_to_fit();
                    FINISHED = true;
                    0
                } else {
                    STATUS_MORE
                }
            }
            Ok(None) => STATUS_NEED_INPUT,
            Err(code) => {
                ERROR_CODE = code;
                STATUS_ERROR
            }
        }
    }
}

pub(crate) extern "C" fn extract_input_supply(len: u32, success: u32) -> u32 {
    unsafe {
        if !STARTED || FINISHED || !PENDING || crate::SUPPLIED || len != PENDING_LEN || success == 0
        {
            return STATUS_ERROR;
        }
        crate::INPUT_LEN = len as usize;
        crate::SUPPLIED = true;
        0
    }
}

fn try_one_step() -> Result<Option<(usize, [u8; 32])>, u32> {
    unsafe {
        DEBUG_STAGE = 1;

        // Keep nod's parsed disc and partition alive between host range requests.
        // Reconstructing them for every range replays partition discovery and FST
        // setup hundreds of times, consuming most of the production host's fuel.
        if DISC.is_none() {
            match DiscReader::new_stream(Box::new(RangeFeedStream), &DiscOptions::default()) {
                Ok(disc) => DISC = Some(disc),
                Err(_) if PENDING => return Ok(None),
                Err(_) => return Err(2),
            }
        }
        let disc = DISC.as_ref().expect("disc initialized");
        if disc.header().game_id_str() != "RMGE01" {
            return Err(3);
        }
        if PARTITION.is_none() {
            match disc.open_partition_kind(
                PartitionKind::Data,
                &PartitionOptions {
                    validate_hashes: false,
                },
            ) {
                Ok(partition) => PARTITION = Some(partition),
                Err(_) if PENDING => return Ok(None),
                Err(_) => return Err(2),
            }
        }
        let partition = PARTITION.as_mut().expect("partition initialized");

        if PARTITION_META.is_none() {
            match partition.meta() {
                Ok(meta) => PARTITION_META = Some(meta),
                Err(_) if PENDING => return Ok(None),
                Err(_) => return Err(2),
            }
        }
        DEBUG_STAGE = 2;
        META_READY = true;

        if FILE_ORDER.is_empty() {
            let meta = PARTITION_META
                .as_ref()
                .expect("partition metadata initialized");
            let fst = meta.fst().map_err(|_| 4u32)?;
            let mut order = Vec::with_capacity(SOURCE_CATALOG.len());
            for (i, source) in SOURCE_CATALOG.iter().enumerate() {
                let path = source.path.strip_prefix("../").unwrap_or(source.path);
                let priority = if source.dol {
                    0
                } else if source.path.starts_with("StageData/") {
                    1
                } else if WANTED_STAGE_ARCS
                    .binary_search_by(|wanted| wanted.as_str().cmp(source.path))
                    .is_ok()
                {
                    2
                } else {
                    3
                };
                if source.dol {
                    order.push((priority, 0u64, i));
                } else {
                    let Some((_, node)) = fst.find(&format!("/{path}")) else {
                        ERROR_CODE = 4;
                        return Err(4);
                    };
                    order.push((priority, node.offset(true), i));
                }
            }
            order.sort_by_key(|item| (item.0, item.1));
            FILE_ORDER.extend(order.into_iter().map(|item| item.2));
        }
        if NEXT_FILE >= FILE_ORDER.len() {
            return Ok(None);
        }

        let source_index = FILE_ORDER[NEXT_FILE];
        let source = &SOURCE_CATALOG[source_index];
        if source.dol {
            let dol = Arc::clone(
                &PARTITION_META
                    .as_ref()
                    .expect("partition metadata initialized")
                    .raw_dol,
            );
            BYTES_DONE += dol.len() as u64;
            MAIN_DOL = Some(Arc::clone(&dol));
            return Ok(Some((NEXT_FILE, Sha256::digest(dol.as_ref()).into())));
        }

        if !FILE_STARTED {
            let meta = PARTITION_META
                .as_ref()
                .expect("partition metadata initialized");
            let fst = meta.fst().map_err(|_| 4u32)?;
            let path = source.path.strip_prefix("../").unwrap_or(source.path);
            let Some((_, node)) = fst.find(&format!("/{path}")) else {
                ERROR_CODE = 4;
                return Err(4);
            };
            partition
                .seek(io::SeekFrom::Start(node.offset(true)))
                .map_err(|_| 2u32)?;
            FILE_REMAINING = node.length() as u64;
            CAPTURING_EFFECT = source.path == "ParticleData/Effect.arc";
            CAPTURING_MARIO = source.path == "ObjectData/Mario.arc";
            CAPTURING_MARIO_ANIME = source.path == "ObjectData/MarioAnime.arc";
            CAPTURING_BEE_WING = source.path == "ObjectData/BeeMarioWing.arc";
            CAPTURING_AUDIO = WANTED_AUDIO_SOURCES
                .binary_search_by(|path| path.as_str().cmp(source.path))
                .is_ok();
            CAPTURING_FIGURE_ARC = WANTED_FIGURE_ARCS
                .binary_search_by(|path| path.as_str().cmp(source.path))
                .is_ok();
            if CAPTURING_FIGURE_ARC {
                FIGURE_ARC_PATH.clear();
                FIGURE_ARC_PATH.push_str(source.path);
                CURRENT_FIGURE_ARC.clear();
            }
            PROCESS_TEXTURE_ARC = WANTED_TEXTURE_ARCS
                .binary_search_by(|path| path.as_str().cmp(source.path))
                .is_ok();
            CAPTURING_TEXTURE_ARC = PROCESS_TEXTURE_ARC && !CAPTURING_FIGURE_ARC;
            CAPTURING_STAGE_ARC = !CAPTURING_FIGURE_ARC
                && WANTED_STAGE_ARCS
                    .binary_search_by(|path| path.as_str().cmp(source.path))
                    .is_ok();
            if CAPTURING_STAGE_ARC {
                STAGE_ARC_PATH.clear();
                STAGE_ARC_PATH.push_str(source.path);
                CURRENT_STAGE_ARC.clear();
            }
            CAPTURING_STAGE_DATA = source.path.starts_with("StageData/");
            if CAPTURING_STAGE_DATA {
                STAGE_DATA_PATH.clear();
                STAGE_DATA_PATH.push_str(source.path);
            }
            for (capture, buffer) in [
                (CAPTURING_EFFECT, &mut EFFECT_ARC),
                (CAPTURING_MARIO, &mut MARIO_ARC),
                (CAPTURING_MARIO_ANIME, &mut MARIO_ANIME_ARC),
                (CAPTURING_BEE_WING, &mut BEE_WING_ARC),
                (CAPTURING_AUDIO, &mut CURRENT_AUDIO),
                (CAPTURING_FIGURE_ARC, &mut CURRENT_FIGURE_ARC),
                (CAPTURING_TEXTURE_ARC, &mut CURRENT_TEXTURE_ARC),
                (CAPTURING_STAGE_DATA, &mut CURRENT_STAGE_DATA_ARC),
                (CAPTURING_STAGE_ARC, &mut CURRENT_STAGE_ARC),
            ] {
                if capture {
                    buffer.clear();
                    if usize::try_from(FILE_REMAINING)
                        .ok()
                        .and_then(|size| buffer.try_reserve_exact(size).ok())
                        .is_none()
                    {
                        return Err(6);
                    }
                }
            }
            FILE_STARTED = true;
            FILE_HASH = Some(Sha256::new());
        }

        let mut chunk = [0u8; 64 * 1024];
        while FILE_REMAINING > 0 {
            let want = cmp::min(FILE_REMAINING, chunk.len() as u64) as usize;
            match partition.read(&mut chunk[..want]) {
                Ok(0) => {
                    if PENDING {
                        return Ok(None);
                    }
                    return Err(2);
                }
                Ok(n) => {
                    FILE_HASH
                        .as_mut()
                        .expect("file hash initialized")
                        .update(&chunk[..n]);
                    if CAPTURING_EFFECT {
                        EFFECT_ARC.extend_from_slice(&chunk[..n]);
                    }
                    if CAPTURING_MARIO {
                        MARIO_ARC.extend_from_slice(&chunk[..n]);
                    }
                    if CAPTURING_MARIO_ANIME {
                        MARIO_ANIME_ARC.extend_from_slice(&chunk[..n]);
                    }
                    if CAPTURING_BEE_WING {
                        BEE_WING_ARC.extend_from_slice(&chunk[..n]);
                    }
                    if CAPTURING_AUDIO {
                        CURRENT_AUDIO.extend_from_slice(&chunk[..n]);
                    }
                    if CAPTURING_FIGURE_ARC {
                        CURRENT_FIGURE_ARC.extend_from_slice(&chunk[..n]);
                    }
                    if CAPTURING_TEXTURE_ARC {
                        CURRENT_TEXTURE_ARC.extend_from_slice(&chunk[..n]);
                    }
                    if CAPTURING_STAGE_DATA {
                        CURRENT_STAGE_DATA_ARC.extend_from_slice(&chunk[..n]);
                    }
                    if CAPTURING_STAGE_ARC {
                        CURRENT_STAGE_ARC.extend_from_slice(&chunk[..n]);
                    }
                    BYTES_DONE += n as u64;
                    FILE_REMAINING -= n as u64;
                }
                Err(_) if PENDING => return Ok(None),
                Err(_) => return Err(2),
            }
        }

        let digest = FILE_HASH
            .take()
            .expect("file hash initialized")
            .finalize()
            .into();
        FILE_STARTED = false;
        Ok(Some((NEXT_FILE, digest)))
    }
}

fn initialize_stages() -> Result<(), ()> {
    unsafe {
        let mut zone_archives = std::collections::BTreeMap::<String, Vec<u8>>::new();
        for (path, bytes) in &STAGE_DATA_ARCS {
            let Some(relative) = path.strip_prefix("StageData/") else {
                continue;
            };
            if relative.contains('/') || !relative.ends_with(".arc") {
                continue;
            }
            let Some(name) = relative.strip_suffix(".arc") else {
                continue;
            };
            zone_archives.insert(name.to_owned(), bytes.clone());
        }
        for pack in crate::asset_packs::EMBEDDED_PACKS
            .iter()
            .filter(|pack| pack.name.starts_with("stages/"))
        {
            let recipe = crate::stage_recipe::StageRecipe::load(pack.name).map_err(|_| ())?;
            let scenario_path =
                format!("StageData/{}/{}Scenario.arc", recipe.galaxy, recipe.galaxy);
            let scenario = STAGE_DATA_ARCS.get(&scenario_path).ok_or(())?;
            let stage =
                crate::stage_galaxy::StageGalaxy::parse(&recipe.galaxy, scenario, &zone_archives)
                    .map_err(|_| ())?;
            for archive in recipe.source_archives() {
                *STAGE_SOURCE_REFS.entry(archive).or_default() += 1;
            }
            PENDING_STAGES.push(PendingStage { recipe, stage });
        }
        STAGES_INITIALIZED = true;
    }
    Ok(())
}

fn process_ready_stages() -> Result<(), ()> {
    unsafe {
        let mut index = 0;
        while index < PENDING_STAGES.len() {
            let ready = PENDING_STAGES[index]
                .recipe
                .source_archives()
                .iter()
                .all(|path| {
                    STAGE_ARCHIVES.contains_key(path)
                        || FIGURE_ARCS.iter().any(|(saved, _)| saved == path)
                });
            if !ready {
                index += 1;
                continue;
            }
            let task = PENDING_STAGES.swap_remove(index);
            let files = crate::stage_assets::geometry_files(&task.recipe, &task.stage, |path| {
                STAGE_ARCHIVES.get(path).cloned().or_else(|| {
                    FIGURE_ARCS
                        .iter()
                        .find(|(saved, _)| saved == path)
                        .map(|(_, bytes)| bytes.clone())
                })
            })
            .map_err(|_| ())?;
            GENERATED_STAGE_ASSETS.extend(files);
            for archive in task.recipe.source_archives() {
                let Some(remaining) = STAGE_SOURCE_REFS.get_mut(&archive) else {
                    return Err(());
                };
                *remaining = remaining.checked_sub(1).ok_or(())?;
                if *remaining == 0 {
                    STAGE_SOURCE_REFS.remove(&archive);
                    STAGE_ARCHIVES.remove(&archive);
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn take_generated_assets() -> (Vec<(String, Vec<u8>)>, Vec<(Vec<String>, Vec<u8>)>) {
    unsafe {
        (
            core::mem::take(&mut GENERATED_ASSETS),
            core::mem::take(&mut GENERATED_TEXTURES),
        )
    }
}

/// Release the disc parser, FST, and range caches before asset generation.
/// Those structures are no longer needed after the final source hash is
/// checked, and their allocations can be reused for generated intermediates.
fn release_extraction_state() {
    unsafe {
        DISC = None;
        PARTITION = None;
        PARTITION_META = None;
        FILE_HASH = None;
        META_CACHE = Vec::new();
        FILE_CACHE = Vec::new();
        FILE_ORDER = Vec::new();
        META_CACHE_BYTES = 0;
        FILE_CACHE_BYTES = 0;
        crate::INPUT = Vec::new();
        crate::INPUT_LEN = 0;
        crate::INPUT_CAPACITY = 0;
        crate::SUPPLIED = false;
    }
}

fn make_cache_room(cache: &mut Vec<CachedRange>, used: &mut usize, incoming: usize, limit: usize) {
    let keep = limit.saturating_sub(incoming);
    while *used > keep && !cache.is_empty() {
        let old = cache.remove(0);
        *used -= old.data.len();
    }
}

fn insert_range(cache: &mut Vec<CachedRange>, used: &mut usize, range: CachedRange) {
    *used += range.data.len();
    cache.push(range);
}

#[derive(Clone)]
struct RangeFeedStream;

impl DiscStream for RangeFeedStream {
    fn read_exact_at(&mut self, out: &mut [u8], offset: u64) -> io::Result<()> {
        unsafe {
            let end = offset.checked_add(out.len() as u64).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "read offset overflow")
            })?;
            if end > IMAGE_SIZE {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            if copy_cached(&META_CACHE, offset, out) || copy_cached(&FILE_CACHE, offset, out) {
                return Ok(());
            }
            if !PENDING {
                let cap = if META_READY {
                    crate::INPUT_CAPACITY as u64
                } else {
                    cmp::min(crate::INPUT_CAPACITY as u64, 8 * 1024 * 1024)
                };
                let alignment = cmp::min(cap, RANGE_ALIGNMENT);
                let start = offset / alignment * alignment;
                PENDING_OFFSET = start;
                PENDING_LEN = cmp::min(cap, IMAGE_SIZE - start) as u32;
                PENDING_META = !META_READY;
                crate::OFFSET = start;
                PENDING = true;
            }
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "range input required",
            ))
        }
    }

    fn stream_len(&mut self) -> io::Result<u64> {
        unsafe { Ok(IMAGE_SIZE) }
    }
}

unsafe fn copy_cached(cache: &[CachedRange], offset: u64, out: &mut [u8]) -> bool {
    let end = offset + out.len() as u64;
    let Some(range) = cache
        .iter()
        .find(|range| offset >= range.offset && end <= range.offset + range.data.len() as u64)
    else {
        return false;
    };
    let start = (offset - range.offset) as usize;
    out.copy_from_slice(&range.data[start..start + out.len()]);
    true
}

pub(crate) fn extract_error_code() -> u32 {
    unsafe { ERROR_CODE }
}
pub(crate) fn extract_files_done() -> u32 {
    unsafe { FILES_DONE }
}
pub(crate) fn extract_files_total() -> u32 {
    SOURCE_CATALOG.len() as u32
}
pub(crate) fn extract_pending_offset_lo() -> u32 {
    unsafe { PENDING_OFFSET as u32 }
}
pub(crate) fn extract_pending_offset_hi() -> u32 {
    unsafe { (PENDING_OFFSET >> 32) as u32 }
}
pub(crate) fn extract_pending_len() -> u32 {
    unsafe { PENDING_LEN }
}

#[allow(dead_code)]
fn _assert_stream_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<RangeFeedStream>();
}
