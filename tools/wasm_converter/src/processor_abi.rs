//! GWRG processor ABI v1 plus the optional range-input v1 exports.

use crate::{extractor, STATUS_MORE};

static mut STREAM_SIZE: u64 = 0;
static mut STREAM_COUNT: u32 = 0;
static mut ALLOCATIONS: Vec<Box<[u8]>> = Vec::new();
static mut COPIED_INPUTS: Vec<Box<[u8]>> = Vec::new();
static mut ERROR: [u8; 96] = [0; 96];
static mut ERROR_LEN: usize = 0;
static mut CONSTRUCTED_ASSETS: Vec<(String, Vec<u8>)> = Vec::new();
#[cfg(feature = "assetc-linked")]
type GeneratedAssets = (Vec<(String, Vec<u8>)>, Vec<(Vec<String>, Vec<u8>)>);
#[cfg(feature = "assetc-linked")]
static mut PENDING_ASSETS: Option<GeneratedAssets> = None;
#[cfg(feature = "assetc-linked")]
static mut ASSETC_PENDING: bool = false;

static STAGE: &[u8] = b"Reading and verifying selected game files";
#[cfg(feature = "assetc-linked")]
static COMPILER_STAGE: &[u8] = b"Converting assets and assembling final XIP";
static EMPTY: &[u8] = b"";

#[cfg(not(feature = "assetc-linked"))]
fn pack_generated_assets(
    files: Vec<(String, Vec<u8>)>,
    textures: Vec<(Vec<String>, Vec<u8>)>,
) -> Result<Vec<u8>, PackError> {
    let texture_files = textures.iter().map(|(paths, _)| paths.len()).sum::<usize>();
    let mut entries = Vec::with_capacity(
        files
            .len()
            .checked_add(texture_files)
            .ok_or(PackError::Size)?,
    );
    let mut payload_len = 0usize;
    for (name, bytes) in &files {
        entries.push((name, payload_len, bytes.len()));
        payload_len = payload_len
            .checked_add(bytes.len())
            .ok_or(PackError::Size)?;
    }
    for (paths, bytes) in &textures {
        let offset = payload_len;
        for name in paths {
            entries.push((name, offset, bytes.len()));
        }
        payload_len = payload_len
            .checked_add(bytes.len())
            .ok_or(PackError::Size)?;
    }
    let index = serde_json::to_vec(&entries).map_err(|_| PackError::Index)?;
    let index_len = u32::try_from(index.len()).map_err(|_| PackError::Size)?;
    let total_len = 4usize
        .checked_add(index.len())
        .and_then(|size| size.checked_add(payload_len))
        .ok_or(PackError::Size)?;
    let mut packed = Vec::new();
    packed
        .try_reserve_exact(total_len)
        .map_err(|_| PackError::Allocation { bytes: total_len })?;
    packed.extend_from_slice(&index_len.to_le_bytes());
    packed.extend_from_slice(&index);
    for (_, bytes) in files {
        packed.extend_from_slice(&bytes);
    }
    for (_, bytes) in textures {
        packed.extend_from_slice(&bytes);
    }
    Ok(packed)
}

#[cfg(not(feature = "assetc-linked"))]
#[derive(Debug)]
enum PackError {
    Index,
    Size,
    Allocation { bytes: usize },
}

fn set_error(message: &[u8]) {
    unsafe {
        ERROR.fill(0);
        let len = core::cmp::min(message.len(), ERROR.len());
        ERROR[..len].copy_from_slice(&message[..len]);
        ERROR_LEN = len;
    }
}

fn input_error(code: u32) -> &'static [u8] {
    match code {
        1 => b"Invalid ISO range",
        2 => b"Could not read the disc or data partition",
        3 => b"Expected Super Mario Galaxy USA (RMGE01)",
        4 => b"A source manifest file was missing from the disc",
        5 => b"A source file did not match the manifest SHA-256",
        6 => b"Could not construct procedural figure assets",
        7 => b"Could not generate GX texture images",
        9 => b"Could not generate AST audio assets",
        _ => b"Wii ISO extraction failed",
    }
}

fn append_bytes(dst: &mut [u8], len: &mut usize, bytes: &[u8]) {
    let count = core::cmp::min(bytes.len(), dst.len().saturating_sub(*len));
    dst[*len..*len + count].copy_from_slice(&bytes[..count]);
    *len += count;
}

fn set_extraction_error() {
    let code = extractor::extract_error_code();
    if code < 10_000 {
        set_error(input_error(code));
        return;
    }
    let diagnostic = code - 10_000;
    let recipe = diagnostic / 32;
    let stage = diagnostic % 32;
    let description = match stage {
        1 => b"archive model lookup failed" as &[u8],
        2 => b"J3D model parsing failed",
        3 => b"TEV material parsing failed",
        4 => b"generated OBJ parsing failed",
        5 => b"high mesh preparation failed",
        6 => b"material texture decoding failed",
        7 => b"raster job serialization failed",
        9 => b"rendered image downsampling failed",
        10 => b"residual correction failed",
        11 => b"PNG encoding failed",
        12 => b"recipe metadata is invalid",
        13 => b"C rasterizer rejected the job",
        14 => b"C rasterizer arena exhausted",
        15 => b"could not allocate the rasterizer arena",
        16 => b"C rasterizer returned invalid output",
        17 => b"geometry part IDs could not be generated",
        18 => b"BVA shape visibility could not be loaded",
        19 => b"BCK pose could not be loaded",
        20 => b"BRK material color animation could not be loaded",
        21 => b"BTP material texture animation could not be loaded",
        22 => b"DOL material color could not be loaded",
        23 => b"source texture for reprojection could not be loaded",
        24 => b"base geometry texture could not be generated",
        25 => b"animated material texture mapping failed",
        26 => b"base texture recipe did not produce its requested image",
        27 => b"base texture dimensions do not match the animated variant",
        28 => b"secondary source archive was not loaded",
        29 => b"source model lookup failed",
        30 => b"specialized texture input was not found",
        31 => b"specialized texture generation failed",
        _ => b"unknown geometry bake failure",
    };
    let mut message = [0u8; 96];
    let mut len = 0;
    append_bytes(&mut message, &mut len, b"Geometry texture recipe ");
    let mut number = itoa::Buffer::new();
    append_bytes(&mut message, &mut len, number.format(recipe).as_bytes());
    append_bytes(&mut message, &mut len, b": ");
    append_bytes(&mut message, &mut len, description);
    set_error(&message[..len]);
}

#[no_mangle]
pub extern "C" fn alloc(len: u32) -> u32 {
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(len as usize).is_err() {
        return 0;
    }
    bytes.resize(len as usize, 0);
    let boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_ptr() as u32;
    unsafe {
        ALLOCATIONS.push(boxed);
    }
    ptr
}

#[no_mangle]
pub extern "C" fn input_clear() {
    unsafe {
        COPIED_INPUTS.clear();
        STREAM_SIZE = 0;
        STREAM_COUNT = 0;
    }
}

#[no_mangle]
pub extern "C" fn input_add(ptr: u32, len: u32) -> u32 {
    if !valid_memory_range(ptr, len) {
        return u32::MAX;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let copied = bytes.to_vec().into_boxed_slice();
    unsafe {
        let index = COPIED_INPUTS.len() as u32;
        COPIED_INPUTS.push(copied);
        index
    }
}

fn valid_memory_range(ptr: u32, len: u32) -> bool {
    let end = ptr as u64 + len as u64;
    end >= ptr as u64 && end <= memory_bytes()
}

#[cfg(target_arch = "wasm32")]
fn memory_bytes() -> u64 {
    core::arch::wasm32::memory_size(0) as u64 * 65_536
}

#[cfg(not(target_arch = "wasm32"))]
fn memory_bytes() -> u64 {
    u32::MAX as u64
}

#[no_mangle]
pub extern "C" fn run(_flags: u32) -> u32 {
    set_error(b"This converter needs the stepped range-input path");
    6
}

#[no_mangle]
pub extern "C" fn run_begin(flags: u32) -> u32 {
    unsafe {
        ERROR_LEN = 0;
        CONSTRUCTED_ASSETS.clear();
        #[cfg(feature = "assetc-linked")]
        {
            PENDING_ASSETS = None;
            ASSETC_PENDING = false;
        }
        if flags != 0 || STREAM_COUNT != 1 || STREAM_SIZE != 4_699_979_776 {
            set_error(b"Expected one USA ISO stream and no options");
            return 7;
        }
        let lo = STREAM_SIZE as u32;
        let hi = (STREAM_SIZE >> 32) as u32;
        let status = extractor::extract_begin(lo, hi, crate::EXTRACT_BUFFER_MIB);
        if status != 0 {
            set_error(b"Could not initialize the bounded ISO extractor");
            return status;
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn run_step() -> u32 {
    #[cfg(feature = "assetc-linked")]
    unsafe {
        if ASSETC_PENDING {
            ASSETC_PENDING = false;
            ERROR_LEN = 0;
            let Some((files, textures)) = PENDING_ASSETS.take() else {
                set_error(b"Generated asset inputs were lost before compilation");
                return 4;
            };
            match crate::wasm_assetc::compile(files, textures) {
                Ok(xip) => {
                    CONSTRUCTED_ASSETS = vec![("smg_assets.xip".into(), xip)];
                    return 0;
                }
                Err(message) => {
                    set_error(message.as_bytes());
                    return 4;
                }
            }
        }
    }

    let status = extractor::extract_step();
    if status == 3 {
        set_extraction_error();
    } else if status == 0 {
        unsafe {
            ERROR_LEN = 0;
            let (files, textures) = extractor::take_generated_assets();
            #[cfg(feature = "assetc-linked")]
            {
                PENDING_ASSETS = Some((files, textures));
                ASSETC_PENDING = true;
                return STATUS_MORE;
            }
            #[cfg(not(feature = "assetc-linked"))]
            match pack_generated_assets(files, textures) {
                Ok(packed) => CONSTRUCTED_ASSETS = vec![("generated_assets.pak".into(), packed)],
                Err(PackError::Allocation { bytes }) => {
                    let mut message = [0u8; 96];
                    let mut len = 0;
                    append_bytes(&mut message, &mut len, b"Could not allocate ");
                    let mut number = itoa::Buffer::new();
                    append_bytes(&mut message, &mut len, number.format(bytes).as_bytes());
                    append_bytes(&mut message, &mut len, b" pack bytes; WASM memory is ");
                    append_bytes(
                        &mut message,
                        &mut len,
                        number.format(memory_bytes()).as_bytes(),
                    );
                    append_bytes(&mut message, &mut len, b" bytes");
                    set_error(&message[..len]);
                    return 4;
                }
                Err(PackError::Index | PackError::Size) => {
                    set_error(b"Generated asset pack index or size overflowed");
                    return 4;
                }
            }
        }
    }
    status
}

#[no_mangle]
pub extern "C" fn stage_count() -> u32 {
    extractor::extract_files_total()
}
#[no_mangle]
pub extern "C" fn stage_index() -> u32 {
    extractor::extract_files_done()
}
#[no_mangle]
pub extern "C" fn stage_name_ptr(_index: u32) -> u32 {
    #[cfg(feature = "assetc-linked")]
    unsafe {
        if ASSETC_PENDING {
            return COMPILER_STAGE.as_ptr() as u32;
        }
    }
    STAGE.as_ptr() as u32
}
#[no_mangle]
pub extern "C" fn stage_name_len(_index: u32) -> u32 {
    #[cfg(feature = "assetc-linked")]
    unsafe {
        if ASSETC_PENDING {
            return COMPILER_STAGE.len() as u32;
        }
    }
    STAGE.len() as u32
}

#[no_mangle]
pub extern "C" fn output_count() -> u32 {
    unsafe { CONSTRUCTED_ASSETS.len() as u32 }
}
#[no_mangle]
pub extern "C" fn output_name_ptr(index: u32) -> u32 {
    unsafe {
        CONSTRUCTED_ASSETS
            .get(index as usize)
            .map_or(0, |(name, _)| name.as_ptr() as u32)
    }
}
#[no_mangle]
pub extern "C" fn output_name_len(index: u32) -> u32 {
    unsafe {
        CONSTRUCTED_ASSETS
            .get(index as usize)
            .map_or(0, |(name, _)| name.len() as u32)
    }
}
#[no_mangle]
pub extern "C" fn output_ptr(index: u32) -> u32 {
    unsafe {
        CONSTRUCTED_ASSETS
            .get(index as usize)
            .map_or(0, |(_, bytes)| bytes.as_ptr() as u32)
    }
}
#[no_mangle]
pub extern "C" fn output_len(index: u32) -> u32 {
    unsafe {
        CONSTRUCTED_ASSETS
            .get(index as usize)
            .map_or(0, |(_, bytes)| bytes.len() as u32)
    }
}

#[no_mangle]
pub extern "C" fn error_ptr() -> u32 {
    core::ptr::addr_of!(ERROR) as u32
}
#[no_mangle]
pub extern "C" fn error_len() -> u32 {
    unsafe { ERROR_LEN as u32 }
}
#[no_mangle]
pub extern "C" fn warnings_ptr() -> u32 {
    EMPTY.as_ptr() as u32
}
#[no_mangle]
pub extern "C" fn warnings_len() -> u32 {
    0
}

#[no_mangle]
pub extern "C" fn input_stream_add(size_lo: u32, size_hi: u32) -> u32 {
    let size = ((size_hi as u64) << 32) | size_lo as u64;
    unsafe {
        if STREAM_COUNT != 0 || size != 4_699_979_776 {
            set_error(b"Expected a single 4,699,979,776-byte USA ISO stream");
            return u32::MAX;
        }
        STREAM_SIZE = size;
        STREAM_COUNT = 1;
    }
    0
}

#[no_mangle]
pub extern "C" fn input_request_index() -> u32 {
    0
}
#[no_mangle]
pub extern "C" fn input_request_offset_lo() -> u32 {
    extractor::extract_pending_offset_lo()
}
#[no_mangle]
pub extern "C" fn input_request_offset_hi() -> u32 {
    extractor::extract_pending_offset_hi()
}
#[no_mangle]
pub extern "C" fn input_request_len() -> u32 {
    extractor::extract_pending_len()
}
#[no_mangle]
pub extern "C" fn input_request_buffer_ptr() -> u32 {
    unsafe { crate::INPUT.as_mut_ptr() as u32 }
}
#[no_mangle]
pub extern "C" fn input_supply(len: u32, success: u32) -> u32 {
    extractor::extract_input_supply(len, success)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_alias_paths_share_one_payload_range() {
        let packed = pack_generated_assets(
            vec![("assets/figure.obj".into(), b"obj".to_vec())],
            vec![(
                vec!["assets/a.png".into(), "assets/b.png".into()],
                b"png".to_vec(),
            )],
        )
        .unwrap();
        let index_len = u32::from_le_bytes(packed[..4].try_into().unwrap()) as usize;
        let index: Vec<(String, usize, usize)> =
            serde_json::from_slice(&packed[4..4 + index_len]).unwrap();
        let payload = &packed[4 + index_len..];
        assert_eq!(payload, b"objpng");
        let a = index
            .iter()
            .find(|(name, _, _)| name == "assets/a.png")
            .unwrap();
        let b = index
            .iter()
            .find(|(name, _, _)| name == "assets/b.png")
            .unwrap();
        assert_eq!((a.1, a.2), (3, 3));
        assert_eq!((b.1, b.2), (3, 3));
    }
}
