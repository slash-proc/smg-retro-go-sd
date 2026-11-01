//! Small no-import ABI spike: request a range above 4 GiB, accept its bytes,
//! and checksum them without copying or mapping the complete input.

const RANGE_LEN: u32 = 64;
const INPUT_LEN: usize = RANGE_LEN as usize;
const PROBE_OFFSET: u64 = (1u64 << 32) + 8_192;

static mut INPUT: [u8; INPUT_LEN] = [0; INPUT_LEN];
static mut FILE_SIZE: u64 = 0;
static mut PHASE: u32 = 0;
static mut SUPPLIED: bool = false;
static mut CHECKSUM: u32 = 2_166_136_261;

const STATUS_MORE: u32 = 1;
const STATUS_NEED_INPUT: u32 = 2;
const STATUS_ERROR: u32 = 3;

#[no_mangle]
pub extern "C" fn abi_version() -> u32 {
    2
}

/// Starts a probe against a file of the given 64-bit size.
#[no_mangle]
pub extern "C" fn run_begin(size_lo: u32, size_hi: u32) -> u32 {
    let size = ((size_hi as u64) << 32) | size_lo as u64;
    if size < PROBE_OFFSET + RANGE_LEN as u64 {
        return STATUS_ERROR;
    }
    unsafe {
        FILE_SIZE = size;
        PHASE = 0;
        SUPPLIED = false;
        CHECKSUM = 2_166_136_261;
        INPUT = [0; INPUT_LEN];
    }
    0
}

/// Returns 2 when the host must satisfy the currently published range.
#[no_mangle]
pub extern "C" fn run_step() -> u32 {
    unsafe {
        match PHASE {
            0 => {
                PHASE = 1;
                STATUS_NEED_INPUT
            }
            1 if SUPPLIED => {
                let input = core::ptr::addr_of!(INPUT).cast::<u8>();
                for i in 0..INPUT_LEN {
                    let byte = input.add(i).read();
                    CHECKSUM = (CHECKSUM ^ byte as u32).wrapping_mul(16_777_619);
                }
                PHASE = 2;
                STATUS_MORE
            }
            1 => STATUS_NEED_INPUT,
            2 => {
                PHASE = 3;
                0
            }
            _ => STATUS_ERROR,
        }
    }
}

#[no_mangle]
pub extern "C" fn request_offset_lo() -> u32 {
    PROBE_OFFSET as u32
}

#[no_mangle]
pub extern "C" fn request_offset_hi() -> u32 {
    (PROBE_OFFSET >> 32) as u32
}

#[no_mangle]
pub extern "C" fn request_len() -> u32 {
    RANGE_LEN
}

/// The host copies exactly `request_len()` bytes here after reading the range.
#[no_mangle]
pub extern "C" fn request_buffer_ptr() -> *mut u8 {
    core::ptr::addr_of_mut!(INPUT).cast::<u8>()
}

#[no_mangle]
pub extern "C" fn input_supply(len: u32, success: u32) -> u32 {
    if len != RANGE_LEN || success == 0 {
        return STATUS_ERROR;
    }
    unsafe {
        if PHASE != 1 || SUPPLIED || FILE_SIZE < PROBE_OFFSET + RANGE_LEN as u64 {
            return STATUS_ERROR;
        }
        SUPPLIED = true;
    }
    0
}

#[no_mangle]
pub extern "C" fn result_checksum() -> u32 {
    unsafe { CHECKSUM }
}
