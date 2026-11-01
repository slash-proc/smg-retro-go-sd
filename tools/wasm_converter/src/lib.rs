//! Import-free WASM stage for streaming a large ISO through bounded memory.
//!
//! The browser Worker satisfies each range request from File.slice(). The
//! module allocates a selectable 64/128/256 MiB input window and keeps the
//! SHA-1 state between windows.

// Export calls are serialized by the host; this instance is not shared across
// threads and does not enable WASM shared memory.
#![allow(static_mut_refs)]

#[cfg(feature = "asset-assembler")]
pub mod asset_assembler;
#[cfg(all(feature = "asset-assembler", not(feature = "assetc-linked")))]
mod asset_assembler_abi;
#[cfg(feature = "assetc-linked")]
mod wasm_assetc;
pub mod asset_formats;
#[cfg(feature = "asset-assembler")]
pub mod asset_packer;
pub mod asset_packs;
pub mod audio_assets;
pub mod audio_jaudio;
pub mod audio_seq;
#[cfg(feature = "processor")]
mod bee_texture;
pub mod fixed_asset_inputs;
pub mod generic_figures;
pub mod geometry_bake;
pub mod geometry_parts;
pub mod j3d_animation;
pub mod j3d_model;
pub mod lookup_tables;
pub mod mario_figure;
#[cfg(feature = "asset-assembler")]
pub mod pose_reanchor;
pub mod procedural_figures;
pub mod procedural_images;
pub mod procedural_sky;
pub mod stage_assets;
pub mod stage_galaxy;
pub mod stage_geometry;
pub mod stage_obj;
pub mod stage_recipe;
pub mod tev;
#[cfg(feature = "asset-assembler")]
pub mod texture_packer;
#[cfg(feature = "processor")]
pub mod textures_bake_core;
pub mod textures_gx;

include!(concat!(env!("OUT_DIR"), "/range_config.rs"));

#[cfg(feature = "processor")]
mod extractor;
#[cfg(feature = "processor")]
mod processor_abi;

const STATUS_MORE: u32 = 1;
const STATUS_NEED_INPUT: u32 = 2;
const STATUS_BAD_INPUT: u32 = 3;
const STATUS_NO_MEMORY: u32 = 4;
const EXPECTED_SIZE: u64 = 4_699_979_776;
const EXPECTED_SHA1: [u8; 20] = [
    0xa3, 0x61, 0x05, 0xc9, 0xfb, 0xff, 0xf6, 0x04, 0x1d, 0xc5, 0xba, 0xbf, 0x43, 0x18, 0xc6, 0xa2,
    0x16, 0xad, 0x47, 0x0a,
];

static mut INPUT: Vec<u8> = Vec::new();
static mut INPUT_LEN: usize = 0;
static mut INPUT_CAPACITY: usize = 0;
static mut FILE_SIZE: u64 = 0;
static mut OFFSET: u64 = 0;
static mut SUPPLIED: bool = false;
static mut STARTED: bool = false;
static mut GAME_ID_OK: bool = false;
static mut DONE: bool = false;
static mut DIGEST: [u8; 20] = [0; 20];
static mut SHA: Sha1 = Sha1::new();

#[no_mangle]
pub extern "C" fn abi_version() -> u32 {
    1
}

/// Starts checking a file. A wrong size is rejected before any range is read.
#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn verify_begin(size_lo: u32, size_hi: u32, buffer_mib: u32) -> u32 {
    let size = ((size_hi as u64) << 32) | size_lo as u64;
    if size != EXPECTED_SIZE {
        return STATUS_BAD_INPUT;
    }
    let capacity = match buffer_mib {
        8 | 64 | 128 | 256 => (buffer_mib as usize) * 1024 * 1024,
        _ => return STATUS_BAD_INPUT,
    };
    unsafe {
        INPUT.clear();
        if INPUT.try_reserve_exact(capacity).is_err() {
            return STATUS_NO_MEMORY;
        }
        INPUT.resize(capacity, 0);
        INPUT_CAPACITY = capacity;
        FILE_SIZE = size;
        OFFSET = 0;
        INPUT_LEN = 0;
        SUPPLIED = false;
        STARTED = true;
        GAME_ID_OK = false;
        DONE = false;
        DIGEST = [0; 20];
        SHA = Sha1::new();
    }
    0
}

/// Returns NEED_INPUT for the next bounded read, MORE after hashing a chunk,
/// zero when complete, and BAD_INPUT on an invalid exchange.
#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn verify_step() -> u32 {
    unsafe {
        if !STARTED || DONE {
            return STATUS_BAD_INPUT;
        }
        if OFFSET == FILE_SIZE {
            if !GAME_ID_OK {
                return STATUS_BAD_INPUT;
            }
            DIGEST = SHA.finish();
            DONE = true;
            return 0;
        }
        if !SUPPLIED {
            return STATUS_NEED_INPUT;
        }
        if INPUT_LEN == 0 || OFFSET + INPUT_LEN as u64 > FILE_SIZE {
            return STATUS_BAD_INPUT;
        }
        if OFFSET == 0 {
            GAME_ID_OK = INPUT_LEN >= 6 && &INPUT[..6] == b"RMGE01";
            if !GAME_ID_OK {
                return STATUS_BAD_INPUT;
            }
        }
        SHA.update(&INPUT[..INPUT_LEN]);
        OFFSET += INPUT_LEN as u64;
        INPUT_LEN = 0;
        SUPPLIED = false;
        STATUS_MORE
    }
}

#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn verify_request_offset_lo() -> u32 {
    unsafe { OFFSET as u32 }
}
#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn verify_request_offset_hi() -> u32 {
    unsafe { (OFFSET >> 32) as u32 }
}
#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn verify_request_len() -> u32 {
    unsafe { core::cmp::min(INPUT_CAPACITY as u64, FILE_SIZE.saturating_sub(OFFSET)) as u32 }
}
#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn verify_request_buffer_ptr() -> u32 {
    unsafe { INPUT.as_mut_ptr() as u32 }
}

#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn verify_input_supply(len: u32, success: u32) -> u32 {
    unsafe {
        let expected =
            core::cmp::min(INPUT_CAPACITY as u64, FILE_SIZE.saturating_sub(OFFSET)) as usize;
        if !STARTED || DONE || SUPPLIED || len as usize != expected || success == 0 {
            return STATUS_BAD_INPUT;
        }
        INPUT_LEN = len as usize;
        SUPPLIED = true;
        0
    }
}

#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn result_digest_ptr() -> u32 {
    core::ptr::addr_of!(DIGEST) as u32
}
#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn result_digest_len() -> u32 {
    20
}
#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn result_matches_expected() -> u32 {
    unsafe { (DONE && DIGEST == EXPECTED_SHA1) as u32 }
}
#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn bytes_processed_lo() -> u32 {
    unsafe { OFFSET as u32 }
}
#[no_mangle]
#[cfg(feature = "verifier-probe")]
pub extern "C" fn bytes_processed_hi() -> u32 {
    unsafe { (OFFSET >> 32) as u32 }
}

struct Sha1 {
    h: [u32; 5],
    tail: [u8; 64],
    tail_len: usize,
    total: u64,
}

impl Sha1 {
    const fn new() -> Self {
        Self {
            h: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0],
            tail: [0; 64],
            tail_len: 0,
            total: 0,
        }
    }

    fn update(&mut self, mut bytes: &[u8]) {
        self.total += bytes.len() as u64;
        if self.tail_len != 0 {
            let n = core::cmp::min(64 - self.tail_len, bytes.len());
            self.tail[self.tail_len..self.tail_len + n].copy_from_slice(&bytes[..n]);
            self.tail_len += n;
            bytes = &bytes[n..];
            if self.tail_len == 64 {
                let block = self.tail;
                self.block(&block);
                self.tail_len = 0;
            }
        }
        let mut chunks = bytes.chunks_exact(64);
        for block in &mut chunks {
            self.block(block.try_into().unwrap());
        }
        let rest = chunks.remainder();
        self.tail[..rest.len()].copy_from_slice(rest);
        self.tail_len = rest.len();
    }

    fn block(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = self.h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a827999),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
    }

    fn finish(&mut self) -> [u8; 20] {
        let bit_len = self.total.wrapping_mul(8);
        self.tail[self.tail_len] = 0x80;
        self.tail_len += 1;
        if self.tail_len > 56 {
            self.tail[self.tail_len..].fill(0);
            let block = self.tail;
            self.block(&block);
            self.tail = [0; 64];
        } else {
            self.tail[self.tail_len..56].fill(0);
        }
        self.tail[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.tail;
        self.block(&block);
        let mut digest = [0; 20];
        for (i, word) in self.h.iter().enumerate() {
            digest[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        digest
    }
}
