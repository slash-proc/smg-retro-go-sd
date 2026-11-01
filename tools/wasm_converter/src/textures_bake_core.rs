//! FFI bridge to the shared C geometry texture renderer.

unsafe extern "C" {
    fn textures_bake_core_set_arena(arena: *mut u8, arena_size: usize);
    fn textures_bake_core_run(input: *const u8, input_size: usize) -> i32;
    fn textures_bake_core_output() -> *const u8;
    fn textures_bake_core_output_len() -> usize;
    fn textures_bake_core_reset();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BakeCoreError {
    HostAllocation,
    InvalidInput,
    ArenaExhausted,
    InvalidOutput,
}

/// Runs the native-compatible texture renderer in a bounded, per-call arena.
/// The size budget covers mesh/BVH allocations plus output-dependent workspaces.
pub fn run(input: &[u8]) -> Result<Vec<u8>, BakeCoreError> {
    if input.len() < 28 || &input[..4] != b"TBK1" {
        return Err(BakeCoreError::InvalidInput);
    }
    let read_i32 = |at: usize| i32::from_le_bytes(input[at..at + 4].try_into().unwrap());
    let width = usize::try_from(read_i32(4)).map_err(|_| BakeCoreError::InvalidInput)?;
    let height = usize::try_from(read_i32(8)).map_err(|_| BakeCoreError::InvalidInput)?;
    let margin = read_i32(12);
    let flags = read_i32(16);
    if width == 0 || height == 0 || width > 4096 || height > 4096 || margin < 0 {
        return Err(BakeCoreError::InvalidInput);
    }
    let pixels = width
        .checked_mul(height)
        .ok_or(BakeCoreError::HostAllocation)?;
    // Normal bakes with margins allocate a 9-neighbor priority queue in
    // margin_adjacent(), in addition to the pixel, mask, and extension buffers.
    // Those buffers are much larger than the serialized job for small meshes.
    let workspace_per_pixel = if flags & 65_536 != 0 {
        40
    } else if margin > 0 {
        160
    } else {
        40
    };
    let capacity = input
        .len()
        .checked_mul(8)
        .and_then(|n| n.checked_add(pixels.checked_mul(workspace_per_pixel)?))
        .and_then(|n| n.checked_add(1 << 20))
        .ok_or(BakeCoreError::HostAllocation)?;
    let mut arena = Vec::new();
    arena
        .try_reserve_exact(capacity)
        .map_err(|_| BakeCoreError::HostAllocation)?;
    arena.resize(capacity, 0);

    unsafe {
        let base = arena.as_mut_ptr();
        let align = (16 - (base as usize & 15)) & 15;
        let arena_ptr = base.add(align);
        textures_bake_core_set_arena(arena_ptr, arena.len() - align);
        let status = textures_bake_core_run(input.as_ptr(), input.len());
        if status != 0 {
            textures_bake_core_reset();
            return Err(match status {
                1 => BakeCoreError::InvalidInput,
                _ => BakeCoreError::ArenaExhausted,
            });
        }
        let output_len = textures_bake_core_output_len();
        let output_ptr = textures_bake_core_output();
        if output_ptr.is_null() && output_len != 0 {
            textures_bake_core_reset();
            return Err(BakeCoreError::InvalidOutput);
        }
        let output = if output_len == 0 {
            Vec::new()
        } else {
            core::slice::from_raw_parts(output_ptr, output_len).to_vec()
        };
        textures_bake_core_reset();
        Ok(output)
    }
}
