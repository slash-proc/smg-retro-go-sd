//! Stage zone placement and object ordering used by stage geometry recipes.

use std::collections::BTreeMap;

use crate::asset_formats::{self, BcsvField, BcsvTable, BcsvValue};

const JMP_DIRS: [&str; 6] = [
    "Placement",
    "MapParts",
    "Start",
    "GeneralPos",
    "ChildObj",
    "Debug",
];
const LAYERS: [&str; 17] = [
    "Common", "LayerA", "LayerB", "LayerC", "LayerD", "LayerE", "LayerF", "LayerG", "LayerH",
    "LayerI", "LayerJ", "LayerK", "LayerL", "LayerM", "LayerN", "LayerO", "LayerP",
];
const PI: f64 = 3.141592653589793;
const RAD: f64 = 180.0 / PI;
const DEG: f64 = PI / 180.0;

const S1: f64 = -1.66666666666666324348e-01;
const S2: f64 = 8.33333333332248946124e-03;
const S3: f64 = -1.98412698298579493134e-04;
const S4: f64 = 2.75573137070700676789e-06;
const S5: f64 = -2.50507602534068634195e-08;
const S6: f64 = 1.58969099521155010221e-10;
const C1: f64 = 4.16666666666666019037e-02;
const C2: f64 = -1.38888888888741095749e-03;
const C3: f64 = 2.48015872894767294178e-05;
const C4: f64 = -2.75573143513906633035e-07;
const C5: f64 = 2.08757232129817482790e-09;
const C6: f64 = -1.13596475577881948265e-11;
const PIO2_1: f64 = 1.57079632673412561417e+00;
const PIO2_1T: f64 = 6.07710050650619224932e-11;
const PIO2_2: f64 = 6.07710050630396597660e-11;
const PIO2_2T: f64 = 2.02226624879595063154e-21;
const PIO2_3: f64 = 2.02226624871116645580e-21;
const PIO2_3T: f64 = 8.47842766036889956997e-32;
const INVPIO2: f64 = 6.36619772367581382433e-01;
const PI_LO: f64 = 1.2246467991473531772e-16;
const PIO2_HI: f64 = 1.57079632679489655800e+00;
const PIO2_LO: f64 = 6.12323399573676603587e-17;
const PIO4_HI: f64 = 7.85398163397448278999e-01;
const PS0: f64 = 1.66666666666666657415e-01;
const PS1: f64 = -3.25565818622400915405e-01;
const PS2: f64 = 2.01212532134862925881e-01;
const PS3: f64 = -4.00555345006794114027e-02;
const PS4: f64 = 7.91534994289814532176e-04;
const PS5: f64 = 3.47933107596021167570e-05;
const QS1: f64 = -2.40339491173441421878e+00;
const QS2: f64 = 2.02094576023350569471e+00;
const QS3: f64 = -6.88283971605453293030e-01;
const QS4: f64 = 7.70381505559019352791e-02;
const ATANHI: [f64; 4] = [
    4.63647609000806093515e-01,
    7.85398163397448278999e-01,
    9.82793723247329054082e-01,
    1.57079632679489655800e+00,
];
const ATANLO: [f64; 4] = [
    2.26987774529616870924e-17,
    3.06161699786838301793e-17,
    1.39033110312309984516e-17,
    6.12323399573676603587e-17,
];
const AT: [f64; 11] = [
    3.33333333333329318027e-01,
    -1.99999999998764832476e-01,
    1.42857142725034663711e-01,
    -1.11111104054623557880e-01,
    9.09088713343650656196e-02,
    -7.69187620504482999495e-02,
    6.66107313738753120669e-02,
    -5.83357013379057348645e-02,
    4.97687799461593236017e-02,
    -3.65315727442169155270e-02,
    1.62858201153657823623e-02,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageGalaxyError {
    InvalidArchive,
    InvalidBcsv,
    MissingZoneList,
    MissingScenario,
}

pub type Matrix = [[f64; 4]; 3];

#[derive(Debug, Clone, PartialEq)]
pub struct ZonePlacement {
    pub zone: String,
    pub parent: Option<usize>,
    pub matrix: Matrix,
    pub raw: ([f64; 3], [f64; 3], [f64; 3]),
    pub scenarios: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageObject {
    pub index: usize,
    pub name: String,
    pub zone: String,
    pub zone_placement: Option<usize>,
    pub world_pos: [f64; 3],
    pub world_rot: [f64; 3],
    pub world_scale: [f64; 3],
    pub raw: ([f64; 3], [f64; 3], [f64; 3]),
    pub scenarios: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageGalaxy {
    pub galaxy: String,
    pub zone_list: Vec<String>,
    pub placements: Vec<ZonePlacement>,
    pub objects: Vec<StageObject>,
}

#[derive(Clone)]
struct Entry {
    layer: usize,
    file: String,
    fields: Vec<BcsvField>,
    row: Vec<BcsvValue>,
}

#[derive(Clone)]
struct Scenario {
    number: i64,
    masks: BTreeMap<String, i64>,
}

fn field_hash(name: &str) -> u32 {
    name.bytes().fold(0u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as u32)
    })
}

fn value<'a>(fields: &[BcsvField], row: &'a [BcsvValue], name: &str) -> Option<&'a BcsvValue> {
    let hash = field_hash(name);
    fields
        .iter()
        .position(|field| field.hash == hash)
        .and_then(|index| row.get(index))
}

fn number(fields: &[BcsvField], row: &[BcsvValue], name: &str, default: f64) -> f64 {
    match value(fields, row, name) {
        Some(BcsvValue::Float(value)) => round_to(*value as f64, 5),
        Some(BcsvValue::Integer(value)) => *value as f64,
        _ => default,
    }
}

fn integer(fields: &[BcsvField], row: &[BcsvValue], name: &str, default: i64) -> i64 {
    match value(fields, row, name) {
        Some(BcsvValue::Integer(value)) => *value,
        Some(BcsvValue::Float(value)) => *value as i64,
        _ => default,
    }
}

fn string(fields: &[BcsvField], row: &[BcsvValue], name: &str) -> Option<String> {
    match value(fields, row, name) {
        Some(BcsvValue::Text(value)) => Some(value.clone()),
        _ => None,
    }
}

fn vec3(fields: &[BcsvField], row: &[BcsvValue], field: &str, default: f64) -> [f64; 3] {
    [
        number(fields, row, &format!("{field}_x"), default),
        number(fields, row, &format!("{field}_y"), default),
        number(fields, row, &format!("{field}_z"), default),
    ]
}

fn raw_vec3(fields: &[BcsvField], row: &[BcsvValue], field: &str, default: f64) -> [f64; 3] {
    [
        raw_number(fields, row, &format!("{field}_x"), default),
        raw_number(fields, row, &format!("{field}_y"), default),
        raw_number(fields, row, &format!("{field}_z"), default),
    ]
}

fn raw_number(fields: &[BcsvField], row: &[BcsvValue], name: &str, default: f64) -> f64 {
    match value(fields, row, name) {
        Some(BcsvValue::Float(value)) => *value as f64,
        Some(BcsvValue::Integer(value)) => *value as f64,
        _ => default,
    }
}

pub(crate) fn round_to(value: f64, decimals: i32) -> f64 {
    let scale = 10u128.pow(decimals as u32);
    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    if exponent == 0x7ff {
        return value;
    }
    let (mantissa, binary_exponent) = if exponent == 0 {
        (fraction as u128, -1074)
    } else {
        (((1u64 << 52) | fraction) as u128, exponent - 1075)
    };
    let numerator = mantissa * scale;
    let rounded = if binary_exponent >= 0 {
        numerator << binary_exponent as u32
    } else {
        let shift = (-binary_exponent) as u32;
        if shift >= 128 {
            0
        } else {
            let quotient = numerator >> shift;
            let remainder = numerator & ((1u128 << shift) - 1);
            let halfway = 1u128 << (shift - 1);
            if remainder > halfway || (remainder == halfway && quotient & 1 != 0) {
                quotient + 1
            } else {
                quotient
            }
        }
    };
    let rounded = (rounded as f64) / (scale as f64);
    if negative {
        -rounded
    } else {
        rounded
    }
}

fn high_word(value: f64) -> i32 {
    (value.to_bits() >> 32) as u32 as i32
}

fn abs_high_word(value: f64) -> u32 {
    high_word(value) as u32 & 0x7fff_ffff
}

fn with_high_word(value: i32) -> f64 {
    f64::from_bits((value as u32 as u64) << 32)
}

fn kernel_sin(x: f64, y: f64, iy: i32) -> f64 {
    if abs_high_word(x) < 0x3e400000 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    if iy == 0 {
        x + v * (S1 + z * r)
    } else {
        x - ((z * (0.5 * y - v * r) - y) - v * S1)
    }
}

fn kernel_cos(x: f64, y: f64) -> f64 {
    let ix = abs_high_word(x);
    if ix < 0x3e400000 {
        return 1.0;
    }
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    if ix < 0x3fd33333 {
        1.0 - (0.5 * z - (z * r - x * y))
    } else {
        let qx = if ix > 0x3fe90000 {
            0.28125
        } else {
            with_high_word((ix as i32) - 0x00200000)
        };
        let hz = 0.5 * z - qx;
        let a = 1.0 - qx;
        a - (hz - (z * r - x * y))
    }
}

fn rem_pio2(x: f64) -> (i32, f64, f64) {
    let hx = high_word(x);
    let ix = hx as u32 & 0x7fff_ffff;
    if ix <= 0x3fe921fb {
        return (0, x, 0.0);
    }
    if ix < 0x4002d97c {
        if hx > 0 {
            let mut z = x - PIO2_1;
            if ix != 0x3ff921fb {
                let y0 = z - PIO2_1T;
                return (1, y0, (z - y0) - PIO2_1T);
            }
            z -= PIO2_2;
            let y0 = z - PIO2_2T;
            return (1, y0, (z - y0) - PIO2_2T);
        }
        let mut z = x + PIO2_1;
        if ix != 0x3ff921fb {
            let y0 = z + PIO2_1T;
            return (-1, y0, (z - y0) + PIO2_1T);
        }
        z += PIO2_2;
        let y0 = z + PIO2_2T;
        return (-1, y0, (z - y0) + PIO2_2T);
    }
    assert!(
        ix <= 0x413921fb,
        "stage transform angle outside fdlibm range"
    );
    let t = x.abs();
    let n = (t * INVPIO2 + 0.5) as i32;
    let fn_ = n as f64;
    let mut r = t - fn_ * PIO2_1;
    let mut w = fn_ * PIO2_1T;
    let j = ix >> 20;
    let mut y0 = r - w;
    let mut i = j as i32 - ((high_word(y0) >> 20) & 0x7ff);
    if i > 16 {
        let t = r;
        w = fn_ * PIO2_2;
        r = t - w;
        w = fn_ * PIO2_2T - ((t - r) - w);
        y0 = r - w;
        i = j as i32 - ((high_word(y0) >> 20) & 0x7ff);
        if i > 49 {
            let t = r;
            w = fn_ * PIO2_3;
            r = t - w;
            w = fn_ * PIO2_3T - ((t - r) - w);
            y0 = r - w;
        }
    }
    let y1 = (r - y0) - w;
    if hx < 0 {
        (-n, -y0, -y1)
    } else {
        (n, y0, y1)
    }
}

pub(crate) fn fd_sin(x: f64) -> f64 {
    let ix = abs_high_word(x);
    if ix <= 0x3fe921fb {
        return kernel_sin(x, 0.0, 0);
    }
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => kernel_sin(y0, y1, 1),
        1 => kernel_cos(y0, y1),
        2 => -kernel_sin(y0, y1, 1),
        _ => -kernel_cos(y0, y1),
    }
}

pub(crate) fn fd_cos(x: f64) -> f64 {
    let ix = abs_high_word(x);
    if ix <= 0x3fe921fb {
        return kernel_cos(x, 0.0);
    }
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => kernel_cos(y0, y1),
        1 => -kernel_sin(y0, y1, 1),
        2 => -kernel_cos(y0, y1),
        _ => kernel_sin(y0, y1, 1),
    }
}

fn fd_atan(x: f64) -> f64 {
    let hx = high_word(x);
    let ix = hx as u32 & 0x7fff_ffff;
    if ix >= 0x44100000 {
        return if hx > 0 {
            ATANHI[3] + ATANLO[3]
        } else {
            -ATANHI[3] - ATANLO[3]
        };
    }
    let idx: i32;
    let mut value = x;
    if ix < 0x3fdc0000 {
        if ix < 0x3e400000 {
            return x;
        }
        idx = -1;
    } else {
        value = x.abs();
        if ix < 0x3ff30000 {
            if ix < 0x3fe60000 {
                idx = 0;
                value = (2.0 * value - 1.0) / (2.0 + value);
            } else {
                idx = 1;
                value = (value - 1.0) / (value + 1.0);
            }
        } else if ix < 0x40038000 {
            idx = 2;
            value = (value - 1.5) / (1.0 + 1.5 * value);
        } else {
            idx = 3;
            value = -1.0 / value;
        }
    }
    let z = value * value;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    if idx < 0 {
        value - value * (s1 + s2)
    } else {
        let result = ATANHI[idx as usize] - ((value * (s1 + s2) - ATANLO[idx as usize]) - value);
        if hx < 0 {
            -result
        } else {
            result
        }
    }
}

fn fd_atan2(y: f64, x: f64) -> f64 {
    if x == 1.0 {
        return fd_atan(y);
    }
    let ys = y.is_sign_negative();
    let xs = x.is_sign_negative();
    if y == 0.0 {
        return if !xs {
            y
        } else if ys {
            -PI
        } else {
            PI
        };
    }
    if x == 0.0 {
        return if ys { -PI / 2.0 } else { PI / 2.0 };
    }
    let k = ((high_word(y) >> 20) & 0x7ff) - ((high_word(x) >> 20) & 0x7ff);
    let z = if k > 60 {
        PI / 2.0 + 0.5 * PI_LO
    } else if xs && k < -60 {
        0.0
    } else {
        fd_atan((y / x).abs())
    };
    match (xs, ys) {
        (false, false) => z,
        (false, true) => -z,
        (true, false) => PI - (z - PI_LO),
        (true, true) => (z - PI_LO) - PI,
    }
}

fn fd_asin(x: f64) -> f64 {
    let hx = high_word(x);
    let ix = hx as u32 & 0x7fff_ffff;
    if ix >= 0x3ff00000 {
        assert!(x == 1.0 || x == -1.0, "stage transform asin outside domain");
        return x * PIO2_HI + x * PIO2_LO;
    }
    let (p, q) = if ix < 0x3fe00000 {
        if ix < 0x3e400000 {
            return x;
        }
        let t = x * x;
        (
            t * (PS0 + t * (PS1 + t * (PS2 + t * (PS3 + t * (PS4 + t * PS5))))),
            1.0 + t * (QS1 + t * (QS2 + t * (QS3 + t * QS4))),
        )
    } else {
        let w = 1.0 - x.abs();
        let t = w * 0.5;
        (
            t * (PS0 + t * (PS1 + t * (PS2 + t * (PS3 + t * (PS4 + t * PS5))))),
            1.0 + t * (QS1 + t * (QS2 + t * (QS3 + t * QS4))),
        )
    };
    if ix < 0x3fe00000 {
        x + x * (p / q)
    } else {
        let t = (1.0 - x.abs()) * 0.5;
        let s = t.sqrt();
        let result = if ix >= 0x3fef3333 {
            let w = p / q;
            PIO2_HI - (2.0 * (s + s * w) - PIO2_LO)
        } else {
            let w = with_high_word(high_word(s));
            let c = (t - w * w) / (s + w);
            let r = p / q;
            let p = 2.0 * s * r - (PIO2_LO - 2.0 * c);
            let q = PIO4_HI - 2.0 * w;
            PIO4_HI - (p - q)
        };
        if hx > 0 {
            result
        } else {
            -result
        }
    }
}

pub(crate) fn trs(pos: [f64; 3], degrees: [f64; 3]) -> Matrix {
    let radians = degrees.map(|value| value * DEG);
    let (sx, cx) = (fd_sin(radians[0]), fd_cos(radians[0]));
    let (sy, cy) = (fd_sin(radians[1]), fd_cos(radians[1]));
    let (sz, cz) = (fd_sin(radians[2]), fd_cos(radians[2]));
    [
        [
            cz * cy,
            cz * sy * sx - sz * cx,
            cz * sy * cx + sz * sx,
            pos[0],
        ],
        [
            sz * cy,
            sz * sy * sx + cz * cx,
            sz * sy * cx - cz * sx,
            pos[1],
        ],
        [-sy, cy * sx, cy * cx, pos[2]],
    ]
}

fn multiply(a: Matrix, b: Matrix) -> Matrix {
    let mut out = [[0.0; 4]; 3];
    for row in 0..3 {
        for column in 0..3 {
            out[row][column] = 0.0
                + a[row][0] * b[0][column]
                + a[row][1] * b[1][column]
                + a[row][2] * b[2][column];
        }
        out[row][3] =
            0.0 + a[row][0] * b[0][3] + a[row][1] * b[1][3] + a[row][2] * b[2][3] + a[row][3];
    }
    out
}

fn rounded_matrix(matrix: Matrix) -> Matrix {
    matrix.map(|row| row.map(|value| round_to(value, 3)))
}

fn euler(matrix: Matrix) -> [f64; 3] {
    let r20 = matrix[2][0].clamp(-1.0, 1.0);
    let y = fd_asin(-r20);
    let (x, z) = if r20.abs() < 0.99999 {
        (
            fd_atan2(matrix[2][1], matrix[2][2]),
            fd_atan2(matrix[1][0], matrix[0][0]),
        )
    } else {
        (fd_atan2(-matrix[1][2], matrix[1][1]), 0.0)
    };
    [x, y, z].map(|value| round_to(value * RAD, 4))
}

fn read_bcsv(bytes: &[u8]) -> Result<BcsvTable, StageGalaxyError> {
    asset_formats::bcsv_decode(bytes).map_err(|_| StageGalaxyError::InvalidBcsv)
}

fn archive_file(archive: &[u8], path: &str) -> Result<Vec<u8>, StageGalaxyError> {
    asset_formats::rarc_find(archive, path).map_err(|_| StageGalaxyError::InvalidArchive)
}

fn zone_names(scenario_archive: &[u8]) -> Result<Vec<String>, StageGalaxyError> {
    let bytes = archive_file(scenario_archive, "ZoneList.bcsv")?;
    let table = read_bcsv(&bytes)?;
    table
        .rows
        .iter()
        .map(|row| string(&table.fields, row, "ZoneName").ok_or(StageGalaxyError::MissingZoneList))
        .collect()
}

fn scenarios(scenario_archive: &[u8], zones: &[String]) -> Result<Vec<Scenario>, StageGalaxyError> {
    let bytes = archive_file(scenario_archive, "ScenarioData.bcsv")?;
    let table = read_bcsv(&bytes)?;
    table
        .rows
        .iter()
        .map(|row| {
            let number = integer(&table.fields, row, "ScenarioNo", -1);
            if number < 0 {
                return Err(StageGalaxyError::MissingScenario);
            }
            let masks = zones
                .iter()
                .map(|zone| {
                    let mask = integer(&table.fields, row, zone, 0);
                    (zone.clone(), (mask * 2) | 1)
                })
                .collect();
            Ok(Scenario { number, masks })
        })
        .collect()
}

fn active_scenarios(scenarios: &[Scenario], active: &[i64], zone: &str, layer: usize) -> Vec<i64> {
    scenarios
        .iter()
        .filter(|scenario| {
            active.contains(&scenario.number)
                && ((scenario.masks.get(zone).copied().unwrap_or(1) >> layer) & 1) != 0
        })
        .map(|scenario| scenario.number)
        .collect()
}

fn read_zone(archive: Option<&[u8]>) -> Result<Vec<Entry>, StageGalaxyError> {
    let Some(archive) = archive else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    // Preserve stages_galaxy.Galaxy.read_zone() ordering: directory first,
    // layer second, then RARC file order and BCSV row order.
    for directory in JMP_DIRS {
        for (layer, layer_name) in LAYERS.iter().enumerate() {
            let path = format!("jmp/{directory}/{layer_name}");
            let files = asset_formats::rarc_listdir(archive, &path)
                .map_err(|_| StageGalaxyError::InvalidArchive)?;
            for (file_name, bytes) in files {
                let table = read_bcsv(&bytes)?;
                entries.extend(table.rows.into_iter().map(|row| Entry {
                    layer,
                    file: file_name.clone(),
                    fields: table.fields.clone(),
                    row,
                }));
            }
        }
    }
    Ok(entries)
}

fn placement_raw(fields: &[BcsvField], row: &[BcsvValue]) -> ([f64; 3], [f64; 3], [f64; 3]) {
    (
        raw_vec3(fields, row, "pos", 0.0),
        raw_vec3(fields, row, "dir", 0.0),
        raw_vec3(fields, row, "scale", 1.0),
    )
}

fn zone_entries(
    scenario_archive: &[u8],
    zone_archives: &BTreeMap<String, Vec<u8>>,
) -> Result<(Vec<String>, BTreeMap<String, Vec<Entry>>), StageGalaxyError> {
    let zones = zone_names(scenario_archive)?;
    let mut entries = BTreeMap::new();
    for zone in &zones {
        entries.insert(
            zone.clone(),
            read_zone(zone_archives.get(zone).map(Vec::as_slice))?,
        );
    }
    Ok((zones, entries))
}

impl StageGalaxy {
    pub fn parse(
        galaxy: &str,
        scenario_archive: &[u8],
        zone_archives: &BTreeMap<String, Vec<u8>>,
    ) -> Result<Self, StageGalaxyError> {
        let (zone_list, zone_entries) = zone_entries(scenario_archive, zone_archives)?;
        let scenario_rows = scenarios(scenario_archive, &zone_list)?;
        let scenario_numbers = scenario_rows
            .iter()
            .map(|scenario| scenario.number)
            .collect::<Vec<_>>();
        let mut result = Self {
            galaxy: galaxy.to_owned(),
            zone_list,
            placements: Vec::new(),
            objects: Vec::new(),
        };
        let identity = trs([0.0; 3], [0.0; 3]);
        result.collect_placements(
            galaxy,
            None,
            identity,
            scenario_numbers.clone(),
            0,
            &scenario_rows,
            &zone_entries,
        );

        let mut instances = vec![(galaxy.to_owned(), None, identity, scenario_numbers)];
        instances.extend(
            result
                .placements
                .iter()
                .enumerate()
                .map(|(index, placement)| {
                    (
                        placement.zone.clone(),
                        Some(index),
                        placement.matrix,
                        placement.scenarios.clone(),
                    )
                }),
        );
        for (zone, placement_index, zone_matrix, active) in instances {
            let Some(entries) = zone_entries.get(&zone) else {
                continue;
            };
            for entry in entries {
                let pos = vec3(&entry.fields, &entry.row, "pos", 0.0);
                let rot = vec3(&entry.fields, &entry.row, "dir", 0.0);
                let scale = vec3(&entry.fields, &entry.row, "scale", 1.0);
                let world = multiply(zone_matrix, trs(pos, rot));
                let object_name = string(&entry.fields, &entry.row, "name").unwrap_or_default();
                result.objects.push(StageObject {
                    index: result.objects.len(),
                    name: object_name,
                    zone: zone.clone(),
                    zone_placement: placement_index,
                    world_pos: [world[0][3], world[1][3], world[2][3]]
                        .map(|value| round_to(value, 3)),
                    world_rot: euler(world),
                    world_scale: scale.map(|value| round_to(value, 3)),
                    raw: placement_raw(&entry.fields, &entry.row),
                    scenarios: active_scenarios(&scenario_rows, &active, &zone, entry.layer),
                });
            }
        }
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_placements(
        &mut self,
        zone: &str,
        parent: Option<usize>,
        parent_matrix: Matrix,
        active: Vec<i64>,
        depth: usize,
        scenarios: &[Scenario],
        entries: &BTreeMap<String, Vec<Entry>>,
    ) {
        if depth > 4 {
            return;
        }
        let Some(zone_entries) = entries.get(zone) else {
            return;
        };
        for entry in zone_entries {
            if !entry.file.eq_ignore_ascii_case("StageObjInfo") {
                continue;
            }
            let Some(child) = string(&entry.fields, &entry.row, "name") else {
                continue;
            };
            let pos = vec3(&entry.fields, &entry.row, "pos", 0.0);
            let rot = vec3(&entry.fields, &entry.row, "dir", 0.0);
            let raw = placement_raw(&entry.fields, &entry.row);
            let world = multiply(parent_matrix, trs(pos, rot));
            let scenarios_for_zone = active_scenarios(scenarios, &active, zone, entry.layer);
            let index = self.placements.len();
            self.placements.push(ZonePlacement {
                zone: child.clone(),
                parent,
                matrix: rounded_matrix(world),
                raw,
                scenarios: scenarios_for_zone.clone(),
            });
            self.collect_placements(
                &child,
                Some(index),
                world,
                scenarios_for_zone,
                depth + 1,
                scenarios,
                entries,
            );
        }
    }
}
