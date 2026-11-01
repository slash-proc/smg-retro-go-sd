//! Deterministic Rust port of `asset_construction/procedural_sky.py`.

const MASK32: u32 = u32::MAX;
const PCG_MULT: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;
const POOL_SIZE: usize = 4;
const PB: u32 = 22;
const SIZE: usize = 256;
const STARS: usize = 420;

fn mix(x: u32, y: u32) -> u32 {
    let value = 0xca01_f9ddu32
        .wrapping_mul(x)
        .wrapping_sub(0x4973_f715u32.wrapping_mul(y));
    value ^ (value >> 16)
}

struct HashMix {
    constant: u32,
}
impl HashMix {
    fn new() -> Self {
        Self {
            constant: 0x43b0_d7e5,
        }
    }
    fn call(&mut self, value: u32) -> u32 {
        let value = value ^ self.constant;
        self.constant = self.constant.wrapping_mul(0x931e_8875);
        let value = value.wrapping_mul(self.constant);
        value ^ (value >> 16)
    }
}

fn seed_state(mut entropy: u64, n_words: usize) -> Vec<u32> {
    let mut words = Vec::new();
    loop {
        words.push(entropy as u32);
        entropy >>= 32;
        if entropy == 0 {
            break;
        }
    }
    let mut hashmix = HashMix::new();
    let mut pool = (0..POOL_SIZE)
        .map(|i| hashmix.call(words.get(i).copied().unwrap_or(0)))
        .collect::<Vec<_>>();
    for src in 0..POOL_SIZE {
        for dst in 0..POOL_SIZE {
            if src != dst {
                let mixed = hashmix.call(pool[src]);
                pool[dst] = mix(pool[dst], mixed);
            }
        }
    }
    for src in POOL_SIZE..words.len() {
        for dst in 0..POOL_SIZE {
            let mixed = hashmix.call(words[src]);
            pool[dst] = mix(pool[dst], mixed);
        }
    }
    let mut out = Vec::with_capacity(n_words);
    let mut h = 0x8b51_f9ddu32;
    for i in 0..n_words {
        let mut value = pool[i % POOL_SIZE] ^ h;
        h = h.wrapping_mul(0x58f3_8ded);
        value = value.wrapping_mul(h);
        out.push(value ^ (value >> 16));
    }
    out
}

struct Pcg64 {
    state: u128,
    inc: u128,
    has32: bool,
    saved32: u32,
}

impl Pcg64 {
    fn new(seed: u64) -> Self {
        let s = seed_state(seed, 8);
        let mut q = [0u64; 4];
        for k in 0..4 {
            q[k] = s[2 * k] as u64 | ((s[2 * k + 1] as u64) << 32);
        }
        let q23 = ((q[2] as u128) << 64) | q[3] as u128;
        let inc = (q23.wrapping_shl(1) | 1) as u128;
        let mut rng = Self {
            state: 0,
            inc,
            has32: false,
            saved32: 0,
        };
        rng.step();
        let init = ((q[0] as u128) << 64) | q[1] as u128;
        rng.state = rng.state.wrapping_add(init);
        rng.step();
        rng
    }

    fn step(&mut self) {
        self.state = self.state.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
    }

    fn next64(&mut self) -> u64 {
        self.step();
        let state = self.state;
        let x = ((state >> 64) ^ state) as u64;
        let rotation = (state >> 122) as u32;
        x.rotate_right(rotation)
    }

    fn next32(&mut self) -> u32 {
        if self.has32 {
            self.has32 = false;
            return self.saved32;
        }
        let value = self.next64();
        self.has32 = true;
        self.saved32 = (value >> 32) as u32;
        value as u32
    }

    fn random(&mut self) -> f64 {
        (self.next64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    fn integer(&mut self, low: u32, high: u32) -> u32 {
        let range = high - low - 1;
        let exclusive = range + 1;
        let mut product = self.next32() as u64 * exclusive as u64;
        let mut left = product as u32;
        if left < exclusive {
            let threshold = (MASK32 - range) % exclusive;
            while left < threshold {
                product = self.next32() as u64 * exclusive as u64;
                left = product as u32;
            }
        }
        low + (product >> 32) as u32
    }
}

fn bicubic(mut x: f64) -> f64 {
    if x < 0.0 {
        x = -x;
    }
    if x < 1.0 {
        return (1.5 * x - 2.5) * x * x + 1.0;
    }
    if x < 2.0 {
        return (((x - 5.0) * x + 8.0) * x - 4.0) * -0.5;
    }
    0.0
}

fn fixed_point(value: f64) -> i64 {
    let scaled = value * (1i64 << PB) as f64;
    if value < 0.0 {
        (-0.5 + scaled) as i64
    } else {
        (0.5 + scaled) as i64
    }
}

fn resample_coeffs(input: usize, output: usize) -> Vec<(usize, Vec<i64>)> {
    let scale = input as f64 / output as f64;
    let fscale = scale.max(1.0);
    let support = 2.0 * fscale;
    let mut result = Vec::with_capacity(output);
    for xx in 0..output {
        let center = (xx as f64 + 0.5) * scale;
        let ss = 1.0 / fscale;
        let mut xmin = (center - support + 0.5) as isize;
        if xmin < 0 {
            xmin = 0;
        }
        let mut xmax = (center + support + 0.5) as usize;
        if xmax > input {
            xmax = input;
        }
        let mut weights = (0..xmax - xmin as usize)
            .map(|x| bicubic((x as f64 + xmin as f64 - center + 0.5) * ss))
            .collect::<Vec<_>>();
        let total: f64 = weights.iter().sum();
        if total != 0.0 {
            for value in &mut weights {
                *value /= total;
            }
        }
        result.push((
            xmin as usize,
            weights.into_iter().map(fixed_point).collect(),
        ));
    }
    result
}

fn resize_l(
    rows: &[Vec<u8>],
    width: usize,
    height: usize,
    target_w: usize,
    target_h: usize,
) -> Vec<Vec<u8>> {
    let half = 1i64 << (PB - 1);
    let horizontal = resample_coeffs(width, target_w);
    let mut tmp = Vec::with_capacity(height);
    for row in rows {
        let mut line = Vec::with_capacity(target_w);
        for (xmin, weights) in &horizontal {
            let sum = weights
                .iter()
                .enumerate()
                .fold(half, |acc, (i, weight)| acc + row[xmin + i] as i64 * weight);
            line.push((sum >> PB).clamp(0, 255) as u8);
        }
        tmp.push(line);
    }
    let vertical = resample_coeffs(height, target_h);
    let mut out = Vec::with_capacity(target_h);
    for (ymin, weights) in vertical {
        let mut accum = vec![half; target_w];
        for (i, weight) in weights.iter().enumerate() {
            for (x, value) in accum.iter_mut().enumerate() {
                *value += tmp[ymin + i][x] as i64 * weight;
            }
        }
        out.push(
            accum
                .into_iter()
                .map(|v| (v >> PB).clamp(0, 255) as u8)
                .collect(),
        );
    }
    out
}

fn noise(rng: &mut Pcg64, size: usize, scale: usize, octaves: usize) -> Vec<Vec<f64>> {
    let mut out = vec![vec![0.0; size]; size];
    let mut amplitude = 1.0;
    for octave in 0..octaves {
        let n = ((size as f64 / scale as f64 * (1 << octave) as f64) as usize).max(2);
        let mut grid = vec![vec![0.0; n + 1]; n + 1];
        for row in &mut grid {
            for value in row {
                *value = rng.random();
            }
        }
        grid[n] = grid[0].clone();
        for row in &mut grid {
            row[n] = row[0];
        }
        let pixels = grid
            .iter()
            .map(|row| row.iter().map(|v| (v * 255.0) as u8).collect())
            .collect::<Vec<Vec<u8>>>();
        let big = size + size / n;
        let enlarged = resize_l(&pixels, n + 1, n + 1, big, big);
        for y in 0..size {
            for x in 0..size {
                out[y][x] += enlarged[y][x] as f64 / 255.0 * amplitude;
            }
        }
        amplitude *= 0.5;
    }
    let peak = out.iter().flatten().copied().fold(0.0f64, f64::max);
    for value in out.iter_mut().flatten() {
        *value /= peak;
    }
    out
}

fn colourise(value: f64, lo: [u8; 3], hi: [u8; 3]) -> [u8; 3] {
    let mut result = [0; 3];
    for c in 0..3 {
        let color = lo[c] as f64 + (hi[c] as f64 - lo[c] as f64) * value;
        result[c] = (color as i32).clamp(0, 255) as u8;
    }
    result
}

/// Return RGB pixel bytes for the deterministic 256 by 256 space texture.
pub fn sky_space() -> Vec<u8> {
    let mut rng = Pcg64::new(7);
    let first = noise(&mut rng, SIZE, 64, 4);
    let second = noise(&mut rng, SIZE, 40, 3);
    let mut pixels = vec![0u8; SIZE * SIZE * 3];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let base = colourise(first[y][x], [6, 8, 30], [30, 26, 80]);
            let value = second[y][x];
            let nebula = if value > 0.55 {
                colourise(value, [0, 0, 0], [90, 40, 120])
            } else {
                [0, 0, 0]
            };
            let p = (y * SIZE + x) * 3;
            for c in 0..3 {
                pixels[p + c] = base[c].saturating_add(nebula[c]);
            }
        }
    }
    for _ in 0..STARS {
        let x = rng.integer(0, SIZE as u32) as i32;
        let y = rng.integer(0, SIZE as u32) as i32;
        let brightness = rng.integer(150, 256) as u8;
        let color = [brightness, brightness, brightness.saturating_add(20)];
        let dots: &[(i32, i32)] = if rng.random() < 0.12 {
            &[(0, -1), (-1, 0), (0, 0), (1, 0), (0, 1)]
        } else {
            &[(0, 0)]
        };
        for &(dx, dy) in dots {
            let (sx, sy) = (x + dx, y + dy);
            if (0..SIZE as i32).contains(&sx) && (0..SIZE as i32).contains(&sy) {
                let p = (sy as usize * SIZE + sx as usize) * 3;
                pixels[p..p + 3].copy_from_slice(&color);
            }
        }
    }
    pixels
}
