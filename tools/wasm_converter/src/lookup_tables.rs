//! Byte-compatible port of `tools/asset_compiler/lookup_tables.c`.

const TAU: f32 = 6.28318530717958647692f32;

#[derive(Default)]
pub struct Tables {
    pub sin_cos: Vec<u8>,
    pub recip8: Vec<u8>,
    pub recip_clip: Vec<u8>,
    pub recip_sprite: Vec<u8>,
    pub recip_area: Vec<u8>,
    pub project: Vec<u8>,
    pub delta: Vec<u8>,
    pub sqrt: Vec<u8>,
    pub atan2: Vec<u8>,
    pub zbin: Vec<u8>,
    pub zdrop: Vec<u8>,
    pub pow: Vec<u8>,
}

fn i32_sat(x: f32) -> i32 {
    if x.is_nan() {
        0
    } else if x >= 2_147_483_648.0 {
        i32::MAX
    } else if x <= -2_147_483_648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

fn i16_sat(x: f32) -> i16 {
    if x.is_nan() {
        0
    } else if x >= 32767.0 {
        i16::MAX
    } else if x <= -32768.0 {
        i16::MIN
    } else {
        x as i16
    }
}

fn fixed(x: f32, frac: i32) -> i32 {
    i32_sat(x * (1i32 << frac) as f32)
}
fn push_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn taylor_sin_cos(sin: bool, x: f32) -> f32 {
    let mut fraction = if sin { x } else { 1.0 };
    let mut fac = if sin { 1.0 } else { 0.0 };
    let neg_x2 = -(x * x);
    let mut sum = 0.0;
    for _ in 0..25 {
        let term = fraction;
        fraction *= neg_x2;
        fraction /= (fac + 1.0) * (fac + 2.0);
        fac += 2.0;
        sum += term;
    }
    sum
}

fn reciprocal(out: &mut Vec<u8>, n: usize, half: i32, numerator_frac: i32, output_frac: i32) {
    for i in 0..n {
        let denominator = i as f32 - half as f32;
        let value = fixed((1i32 << numerator_frac) as f32 / denominator, output_frac);
        push_i32(out, value);
    }
}

fn exponent_of(x: f32) -> i32 {
    ((x.to_bits() & 0x7f80_0000) >> 23) as i32 - 127
}

fn set_exponent(x: f32, e: i32) -> f32 {
    f32::from_bits((x.to_bits() & !0x7f80_0000) | (((e + 127) as u32) << 23))
}

fn copysign_bits(x: f32, sign: f32) -> f32 {
    f32::from_bits((x.to_bits() & 0x7fff_ffff) | (sign.to_bits() & 0x8000_0000))
}

fn approx_fract(x: f32) -> f32 {
    let e = exponent_of(x);
    if e < 0 {
        return x;
    }
    let frac = (x.to_bits() << e) & 0x7f_ffff;
    if frac == 0 {
        return copysign_bits(0.0, x);
    }
    let shift = frac.leading_zeros() - 9 + 1;
    let normal = (frac << shift) & 0x7f_ffff;
    copysign_bits(f32::from_bits(normal | ((127 - shift) << 23)), x)
}

fn approx_trunc(x: f32) -> f32 {
    let e = exponent_of(x);
    if e < 0 {
        return copysign_bits(0.0, x);
    }
    let frac = (x.to_bits() << e) & 0x7f_ffff;
    if frac == 0 {
        return x;
    }
    f32::from_bits(x.to_bits() & !(frac >> e))
}

fn approx_ln(x: f32) -> f32 {
    if (x - 1.0).abs() < f32::EPSILON {
        return 0.0;
    }
    let less = x < 1.0;
    let mut w = if less {
        f32::from_bits(0x7f00_0000u32.wrapping_sub(x.to_bits()))
    } else {
        x
    };
    let e = exponent_of(w) as u32;
    w /= f32::from_bits(w.to_bits() & 0x7f80_0000);
    let poly = -1.7417939f32
        + (2.8212026f32 + (-1.4699568f32 + (0.44717955f32 - 0.056570851f32 * w) * w) * w) * w;
    let r = e as f32 * 0.693147180559945309417f32 + poly;
    if less {
        -r
    } else {
        r
    }
}

fn approx_exp(x: f32) -> f32 {
    if x == 0.0 {
        return 1.0;
    }
    if (x - 1.0).abs() < f32::EPSILON {
        return 2.71828182845904523536f32;
    }
    if (x + 1.0).abs() < f32::EPSILON {
        return 1.0 / 2.71828182845904523536f32;
    }
    let y = x * 1.44269504088896340736f32;
    let fr = approx_fract(y) * 0.693147180559945309417f32;
    let tr = approx_trunc(y);
    let mut total = 1.0;
    for i in (1..=4).rev() {
        total = 1.0 + ((fr / i as f32) * total);
    }
    let e = exponent_of(total) as i64 + i32_sat(tr) as i64;
    if e > i32::MAX as i64 {
        return f32::INFINITY;
    }
    if e < -127 {
        return 0.0;
    }
    if e > 128 {
        return f32::INFINITY;
    }
    set_exponent(total, e as i32)
}

fn approx_powf(x: f32, n: f32) -> f32 {
    approx_exp(n * approx_ln(x))
}
fn cubic_ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x) * (1.0 - x) * (1.0 - x)
}

pub fn build() -> Tables {
    let mut t = Tables::default();
    t.sin_cos.reserve(1024 * 4);
    for i in 0..1024 {
        let a = i as f32 / 1024.0 * TAU;
        push_i16(&mut t.sin_cos, i16_sat(taylor_sin_cos(true, a) * 256.0));
        push_i16(&mut t.sin_cos, i16_sat(taylor_sin_cos(false, a) * 256.0));
    }
    reciprocal(&mut t.recip8, 1 << 15, 1 << 14, 8, 8);
    reciprocal(&mut t.recip_clip, 1 << 15, 1 << 14, 6, 12);
    reciprocal(&mut t.recip_sprite, 1 << 11, 1 << 10, 8, 12);
    reciprocal(&mut t.recip_area, 1 << 15, 0, 8, 12);
    for i in 0..1 << 16 {
        push_i32(&mut t.project, fixed(64.0 / i as f32, 14));
    }
    for i in 0..1 << 15 {
        let mut v = fixed(256.0 / i as f32, 15);
        if (v >> 7) > 256 {
            v = 1 << 15;
        }
        push_i32(&mut t.delta, v);
    }
    for i in 0..4096 {
        push_i16(&mut t.sqrt, i16_sat((i as f32).sqrt() * 256.0));
    }
    for x in 0..64 {
        for y in 0..64 {
            let value = ((x as f32 - 32.0).atan2(y as f32 - 32.0)) * 4096.0;
            push_i16(&mut t.atan2, i16_sat(value));
        }
    }
    let mut zbin = [0u8; 2048];
    for (i, value) in zbin.iter_mut().enumerate() {
        let bin = 128.0 - 128.0 * cubic_ease_out(i as f32 / 4.0 / 256.0);
        *value = if bin < 0.0 {
            0
        } else if bin >= 128.0 {
            127
        } else {
            bin as u8
        };
    }
    t.zbin.extend_from_slice(&zbin);
    for max_bin in 0..=128 {
        let mut drop = i32::MAX;
        if max_bin > 0 {
            if let Some(i) = zbin.iter().position(|&bin| (bin as usize) < max_bin) {
                drop = (i as i32) << 6;
            }
        }
        push_i32(&mut t.zdrop, drop);
    }
    let bases = [0.010f32, 0.05, 0.0000005, 0.8];
    for base in bases {
        for k in 0..=32 {
            push_i32(
                &mut t.pow,
                fixed(approx_powf(base, k as f32 * (1.0 / 256.0)), 8),
            );
        }
    }
    t
}

pub fn named(t: Tables) -> Vec<(String, Vec<u8>)> {
    vec![
        ("lut_sin_cos".into(), t.sin_cos),
        ("lut_recip8".into(), t.recip8),
        ("lut_recip_clip".into(), t.recip_clip),
        ("lut_recip_sprite".into(), t.recip_sprite),
        ("lut_recip_area".into(), t.recip_area),
        ("lut_project".into(), t.project),
        ("lut_delta".into(), t.delta),
        ("lut_sqrt".into(), t.sqrt),
        ("lut_atan2".into(), t.atan2),
        ("lut_zbin".into(), t.zbin),
        ("lut_zdrop".into(), t.zdrop),
        ("lut_pow".into(), t.pow),
    ]
}
