//! In-process bridge to the canonical C asset compiler for the browser
//! processor build. The extractor's generated buffers are borrowed by the C
//! VFS, so the input asset tree is not copied before compilation.

use core::alloc::{GlobalAlloc, Layout};
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
    ptr, slice,
};

use crate::{asset_assembler, asset_packer::PackedAssets};

unsafe extern "C" {
    fn malloc(size: usize) -> *mut u8;
    fn free(ptr: *mut u8);
    fn smg_assetc_vfs_reset();
    fn smg_assetc_vfs_add_input(name: *const u8, data: *const u8, len: usize) -> i32;
    fn smg_assetc_run() -> i32;
    fn smg_assetc_output_count() -> usize;
    fn smg_assetc_output_name(index: usize) -> *const core::ffi::c_char;
    fn smg_assetc_output_data(index: usize) -> *const u8;
    fn smg_assetc_output_len(index: usize) -> usize;
    fn smg_assetc_vfs_release_output_data(index: usize);
}

struct CAllocator;

unsafe impl GlobalAlloc for CAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(16);
        let overhead = align.saturating_add(core::mem::size_of::<usize>());
        let Some(size) = layout.size().max(1).checked_add(overhead) else {
            return ptr::null_mut();
        };
        let raw = unsafe { malloc(size) };
        if raw.is_null() {
            return raw;
        }
        let aligned = ((raw as usize + core::mem::size_of::<usize>() + align - 1) & !(align - 1))
            as *mut u8;
        unsafe { aligned.sub(core::mem::size_of::<usize>()).cast::<usize>().write_unaligned(raw as usize) };
        aligned
    }

    unsafe fn dealloc(&self, pointer: *mut u8, _: Layout) {
        if !pointer.is_null() {
            let raw = unsafe {
                pointer
                    .sub(core::mem::size_of::<usize>())
                    .cast::<usize>()
                    .read_unaligned()
            } as *mut u8;
            unsafe { free(raw) };
        }
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: CAllocator = CAllocator;

pub fn compile(
    files: Vec<(String, Vec<u8>)>,
    textures: Vec<(Vec<String>, Vec<u8>)>,
) -> Result<Vec<u8>, String> {
    unsafe { smg_assetc_vfs_reset() };

    for (name, bytes) in &files {
        add_input(name, bytes)?;
    }
    for (paths, bytes) in &textures {
        for name in paths {
            add_input(name, bytes)?;
        }
    }

    let status = unsafe { smg_assetc_run() };
    drop(files);
    drop(textures);
    if status != 0 {
        unsafe { smg_assetc_vfs_reset() };
        return Err(format!("asset compiler exited with status {status}"));
    }

    let mut assembly = None;
    let mut header = None;
    let mut blobs = BTreeMap::new();
    let output_count = unsafe { smg_assetc_output_count() };
    for index in 0..output_count {
        let result = unsafe { copy_output(index) };
        unsafe { smg_assetc_vfs_release_output_data(index) };
        let (name, bytes) = result?;
        match name.as_str() {
            "assets.s" => {
                assembly = Some(String::from_utf8(bytes).map_err(|_| "asset assembly is not UTF-8")?);
            }
            "assets_gen.h" => {
                header = Some(String::from_utf8(bytes).map_err(|_| "asset header is not UTF-8")?);
            }
            _ if name.ends_with(".bin") => {
                blobs.insert(name, bytes);
            }
            _ => return Err(format!("unexpected asset compiler output {name}")),
        }
    }
    unsafe { smg_assetc_vfs_reset() };

    let mut assets = PackedAssets {
        text: assembly.ok_or("asset compiler did not emit assets.s")?,
        header: header.ok_or("asset compiler did not emit assets_gen.h")?,
        blobs,
    };
    assets.share_tico_bank()?;
    assets.share_canonical_stage_arrays()?;
    assets.share_canonical_pose_descriptors()?;
    assets.share_canonical_texture_overlaps()?;
    assets.share_canonical_whole_arrays()?;
    assets.pool_canonical_textures()?;
    assets.reanchor_canonical_poses()?;

    let xip = asset_assembler::assemble(&assets.text, 0xBEEF_0000, |name| {
        assets
            .blobs
            .remove(name)
            .ok_or_else(|| format!("missing asset compiler blob {name}"))
    })?;
    Ok(xip)
}

unsafe fn copy_output(index: usize) -> Result<(String, Vec<u8>), String> {
    let name = unsafe { smg_assetc_output_name(index) };
    if name.is_null() {
        return Err(format!("asset compiler output {index} has no name"));
    }
    let name = unsafe { CStr::from_ptr(name) }
        .to_str()
        .map_err(|_| "asset compiler output name is not UTF-8")?
        .to_owned();
    let len = unsafe { smg_assetc_output_len(index) };
    let ptr = unsafe { smg_assetc_output_data(index) };
    if ptr.is_null() && len != 0 {
        return Err(format!("asset compiler output {name} has no data"));
    }
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(len)
        .map_err(|_| format!("could not allocate {len} bytes for {name}"))?;
    owned.extend_from_slice(bytes);
    Ok((name, owned))
}

fn add_input(name: &str, bytes: &[u8]) -> Result<(), String> {
    let relative = name.strip_prefix("assets/").unwrap_or(name);
    let name = CString::new(relative).map_err(|_| "asset input path contains a NUL byte")?;
    let added = unsafe {
        smg_assetc_vfs_add_input(name.as_ptr().cast(), bytes.as_ptr(), bytes.len())
    };
    if added == 0 {
        return Err(format!("could not register asset compiler input {relative}"));
    }
    Ok(())
}
