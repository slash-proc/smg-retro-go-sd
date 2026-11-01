//! Small WASM ABI used by the browser worker after the WASI asset compiler.

use crate::asset_assembler;

static mut ALLOCATIONS: Vec<Box<[u8]>> = Vec::new();
static mut ASSEMBLY: String = String::new();
static mut ASSET_HEADER: String = String::new();
static mut BLOBS: Vec<(String, Vec<u8>)> = Vec::new();
static mut OUTPUT: Vec<u8> = Vec::new();
static mut ERROR: [u8; 256] = [0; 256];
static mut ERROR_LEN: usize = 0;

fn set_error(message: &str) {
    unsafe {
        ERROR.fill(0);
        ERROR_LEN = message.len().min(ERROR.len());
        ERROR[..ERROR_LEN].copy_from_slice(&message.as_bytes()[..ERROR_LEN]);
    }
}

#[no_mangle]
pub extern "C" fn asm_alloc(len: u32) -> u32 {
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(len as usize).is_err() {
        return 0;
    }
    bytes.resize(len as usize, 0);
    let boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_ptr() as u32;
    unsafe { ALLOCATIONS.push(boxed) };
    ptr
}

#[no_mangle]
pub extern "C" fn asm_dealloc(ptr: u32) {
    unsafe { ALLOCATIONS.retain(|bytes| bytes.as_ptr() as u32 != ptr) };
}

#[no_mangle]
pub extern "C" fn asm_begin(ptr: u32, len: u32) -> u32 {
    if !valid_range(ptr, len) {
        set_error("Assembly input is outside WASM memory");
        return 1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let Ok(text) = core::str::from_utf8(bytes) else {
        set_error("Assembly input is not UTF-8");
        return 1;
    };
    unsafe {
        ASSEMBLY.clear();
        ASSEMBLY.push_str(text);
        ASSET_HEADER.clear();
        BLOBS.clear();
        OUTPUT.clear();
        ERROR_LEN = 0;
    }
    0
}

#[no_mangle]
pub extern "C" fn asm_set_header(ptr: u32, len: u32) -> u32 {
    if !valid_range(ptr, len) {
        set_error("Asset compiler header is outside WASM memory");
        return 1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let Ok(header) = core::str::from_utf8(bytes) else {
        set_error("Asset compiler header is not UTF-8");
        return 1;
    };
    unsafe {
        ASSET_HEADER.clear();
        ASSET_HEADER.push_str(header);
    }
    0
}

#[no_mangle]
pub extern "C" fn asm_add_blob(name_ptr: u32, name_len: u32, data_ptr: u32, data_len: u32) -> u32 {
    if !valid_range(name_ptr, name_len) || !valid_range(data_ptr, data_len) {
        set_error("Asset blob is outside WASM memory");
        return 1;
    }
    let name_bytes =
        unsafe { core::slice::from_raw_parts(name_ptr as *const u8, name_len as usize) };
    let Ok(name) = core::str::from_utf8(name_bytes) else {
        set_error("Asset blob name is not UTF-8");
        return 1;
    };
    let data = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, data_len as usize) };
    unsafe {
        if BLOBS.iter().any(|(path, _)| path == name) {
            set_error("Duplicate asset blob");
            return 1;
        }
        let mut owned = Vec::new();
        if owned.try_reserve_exact(data.len()).is_err() {
            set_error("Not enough WASM memory for asset blobs");
            return 1;
        }
        owned.extend_from_slice(data);
        BLOBS.push((name.to_owned(), owned));
    }
    0
}

#[no_mangle]
pub extern "C" fn asm_run() -> u32 {
    let result = unsafe {
        asset_assembler::assemble(&ASSEMBLY, 0xBEEF_0000, |name| {
            BLOBS
                .iter()
                .find(|(path, _)| path == name)
                .map(|(_, bytes)| bytes.clone())
                .ok_or_else(|| format!("missing asset compiler blob {name}"))
        })
    };
    match result {
        Ok(bytes) => {
            unsafe { OUTPUT = bytes };
            0
        }
        Err(error) => {
            set_error(&error);
            1
        }
    }
}

/// Apply the first native pack.py passes before flattening sections.
#[no_mangle]
pub extern "C" fn asm_pack() -> u32 {
    let mut assets = unsafe {
        crate::asset_packer::PackedAssets {
            text: core::mem::take(&mut ASSEMBLY),
            header: core::mem::take(&mut ASSET_HEADER),
            blobs: core::mem::take(&mut BLOBS).into_iter().collect(),
        }
    };
    let result = assets
        .share_tico_bank()
        .and_then(|_| assets.share_canonical_stage_arrays())
        .and_then(|_| assets.share_canonical_pose_descriptors())
        .and_then(|_| assets.share_canonical_texture_overlaps())
        .and_then(|_| assets.share_canonical_whole_arrays())
        .and_then(|_| assets.pool_canonical_textures())
        .and_then(|_| assets.reanchor_canonical_poses());
    match result {
        Ok(()) => {
            unsafe {
                ASSEMBLY = assets.text;
                ASSET_HEADER = assets.header;
                BLOBS = assets.blobs.into_iter().collect();
            }
            0
        }
        Err(error) => {
            set_error(&error);
            1
        }
    }
}

#[no_mangle]
pub extern "C" fn asm_output_ptr() -> u32 {
    unsafe { OUTPUT.as_ptr() as u32 }
}

#[no_mangle]
pub extern "C" fn asm_output_len() -> u32 {
    unsafe { OUTPUT.len() as u32 }
}

#[no_mangle]
pub extern "C" fn asm_error_ptr() -> u32 {
    core::ptr::addr_of!(ERROR) as u32
}

#[no_mangle]
pub extern "C" fn asm_error_len() -> u32 {
    unsafe { ERROR_LEN as u32 }
}

#[no_mangle]
pub extern "C" fn asm_clear() {
    unsafe {
        ASSEMBLY.clear();
        ASSET_HEADER.clear();
        BLOBS.clear();
        OUTPUT.clear();
        ALLOCATIONS.clear();
        ERROR_LEN = 0;
    }
}

fn valid_range(ptr: u32, len: u32) -> bool {
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
