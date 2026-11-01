//! Small PNG assets emitted by `asset_construction/procedural.py`.

use crate::procedural_sky::sky_space;

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in bytes {
        a = (a + byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    let start = output.len() - data.len() - 4;
    output.extend_from_slice(&crc32(&output[start..]).to_be_bytes());
}

fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    } else {
        let blocks = data.chunks(u16::MAX as usize).collect::<Vec<_>>();
        for (index, block) in blocks.iter().enumerate() {
            out.push(if index + 1 == blocks.len() { 1 } else { 0 });
            let len = block.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(block);
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn png(width: u32, height: u32, color_type: u8, pixels: &[u8], channels: usize) -> Vec<u8> {
    let row_bytes = width as usize * channels;
    assert_eq!(pixels.len(), row_bytes * height as usize);
    let mut scanlines = Vec::with_capacity(pixels.len() + height as usize);
    for row in pixels.chunks_exact(row_bytes) {
        scanlines.push(0); // PNG filter None.
        scanlines.extend_from_slice(row);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, color_type, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib_stored(&scanlines));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn shadow_png() -> Vec<u8> {
    let size = 32usize;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let dx = 2 * x as i32 + 1 - size as i32;
            let dy = 2 * y as i32 + 1 - size as i32;
            if dx * dx + dy * dy <= (size * size) as i32 {
                rgba[(y * size + x) * 4 + 3] = 255;
            }
        }
    }
    png(size as u32, size as u32, 6, &rgba, 4)
}

/// Return the shadow texture and shared sky texture files from the procedural stage.
pub fn generate() -> Vec<(String, Vec<u8>)> {
    let mut files = vec![("assets/shadow.png".into(), shadow_png())];
    let sky = png(256, 256, 2, &sky_space(), 3);
    for stage in ["honeyhive", "honeyhive_tower", "observatory"] {
        files.push((format!("assets/stages/{stage}/sky.png"), sky.clone()));
    }
    files
}
