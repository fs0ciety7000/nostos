//! Fujifilm maker note: film simulation and the in-camera "recipe" settings.
//!
//! Tag numbers and value codes follow exiftool's `FujiFilm.pm` (12.76).
//! The maker note starts with `FUJIFILM`, then a little-endian u32 giving the
//! IFD offset. All offsets inside are relative to the maker-note start and the
//! byte order is always little-endian, whatever the outer TIFF uses.

use crate::bytes::{Bytes, Endian};
use crate::tiff::Ifd;

mod tag {
    pub const SHARPNESS: u16 = 0x1001;
    pub const WHITE_BALANCE: u16 = 0x1002;
    pub const SATURATION: u16 = 0x1003;
    pub const COLOR_TEMPERATURE: u16 = 0x1005;
    pub const WB_FINE_TUNE: u16 = 0x100a;
    pub const NOISE_REDUCTION: u16 = 0x100e;
    pub const CLARITY: u16 = 0x100f;
    pub const SHADOW_TONE: u16 = 0x1040;
    pub const HIGHLIGHT_TONE: u16 = 0x1041;
    pub const GRAIN_ROUGHNESS: u16 = 0x1047;
    pub const COLOR_CHROME: u16 = 0x1048;
    pub const BW_ADJUSTMENT: u16 = 0x1049;
    pub const BW_MAGENTA_GREEN: u16 = 0x104b;
    pub const GRAIN_SIZE: u16 = 0x104c;
    pub const COLOR_CHROME_FX_BLUE: u16 = 0x104e;
    pub const FILM_MODE: u16 = 0x1401;
    pub const DYNAMIC_RANGE_SETTING: u16 = 0x1402;
    pub const DEVELOPMENT_DYNAMIC_RANGE: u16 = 0x1403;
    pub const D_RANGE_PRIORITY: u16 = 0x1443;
    pub const D_RANGE_PRIORITY_AUTO: u16 = 0x1444;
    pub const D_RANGE_PRIORITY_FIXED: u16 = 0x1445;
}

/// Colour filter applied to a monochrome simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum MonoFilter {
    None,
    Red,
    Yellow,
    Green,
}

/// Film simulation selected in camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", content = "filter"))]
pub enum FilmSimulation {
    Provia,
    Velvia,
    Astia,
    ClassicChrome,
    RealaAce,
    ProNegHi,
    ProNegStd,
    ClassicNeg,
    NostalgicNeg,
    Eterna,
    EternaBleachBypass,
    Acros(MonoFilter),
    Monochrome(MonoFilter),
    Sepia,
    /// A code this crate does not name (older Studio Portrait modes, future simulations).
    Other(u16),
}

impl FilmSimulation {
    /// Resolves the simulation from the `FilmMode` (0x1401) and `Saturation`
    /// (0x1003) codes. Monochrome simulations are only recorded in
    /// `Saturation`; colour ones in `FilmMode`.
    pub fn from_codes(film_mode: Option<u16>, saturation: Option<u16>) -> Option<Self> {
        let mono = |f| match f & 0xF {
            1 => MonoFilter::Red,
            2 => MonoFilter::Yellow,
            3 => MonoFilter::Green,
            _ => MonoFilter::None,
        };
        match saturation {
            Some(s @ 0x500..=0x503) => return Some(Self::Acros(mono(s))),
            Some(s @ 0x300..=0x303) => return Some(Self::Monochrome(mono(s))),
            Some(0x310) => return Some(Self::Sepia),
            _ => {}
        }
        Some(match film_mode? {
            0x000 => Self::Provia,
            0x120 => Self::Astia,
            0x200 | 0x400 => Self::Velvia,
            0x500 => Self::ProNegStd,
            0x501 => Self::ProNegHi,
            0x600 => Self::ClassicChrome,
            0x700 => Self::Eterna,
            0x800 => Self::ClassicNeg,
            0x900 => Self::EternaBleachBypass,
            0xa00 => Self::NostalgicNeg,
            0xb00 => Self::RealaAce,
            other => Self::Other(other),
        })
    }

    /// Name as printed on the camera menu.
    pub fn name(&self) -> String {
        let filter = |f: &MonoFilter| match f {
            MonoFilter::None => "",
            MonoFilter::Red => "+R",
            MonoFilter::Yellow => "+Ye",
            MonoFilter::Green => "+G",
        };
        match self {
            Self::Provia => "Provia".into(),
            Self::Velvia => "Velvia".into(),
            Self::Astia => "Astia".into(),
            Self::ClassicChrome => "Classic Chrome".into(),
            Self::RealaAce => "Reala Ace".into(),
            Self::ProNegHi => "Pro Neg. Hi".into(),
            Self::ProNegStd => "Pro Neg. Std".into(),
            Self::ClassicNeg => "Classic Neg.".into(),
            Self::NostalgicNeg => "Nostalgic Neg.".into(),
            Self::Eterna => "Eterna".into(),
            Self::EternaBleachBypass => "Eterna Bleach Bypass".into(),
            Self::Acros(f) => format!("Acros{}", filter(f)),
            Self::Monochrome(f) => format!("Monochrome{}", filter(f)),
            Self::Sepia => "Sepia".into(),
            Self::Other(code) => format!("Film mode 0x{code:03x}"),
        }
    }

    pub fn is_monochrome(&self) -> bool {
        matches!(self, Self::Acros(_) | Self::Monochrome(_) | Self::Sepia)
    }
}

/// Off / Weak / Strong, used by grain roughness and both Color Chrome effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Strength {
    Off,
    Weak,
    Strong,
}

impl Strength {
    fn from_code(v: i64) -> Option<Self> {
        match v {
            0 => Some(Self::Off),
            32 => Some(Self::Weak),
            64 => Some(Self::Strong),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum GrainSize {
    Off,
    Small,
    Large,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum WhiteBalance {
    Auto,
    AutoWhitePriority,
    AutoAmbiencePriority,
    Daylight,
    Cloudy,
    FluorescentDaylight,
    FluorescentWarmWhite,
    FluorescentCoolWhite,
    Incandescent,
    Flash,
    Underwater,
    Custom(u8),
    Kelvin,
    Other(u16),
}

impl WhiteBalance {
    fn from_code(v: u16) -> Self {
        match v {
            0x0 => Self::Auto,
            0x1 => Self::AutoWhitePriority,
            0x2 => Self::AutoAmbiencePriority,
            0x100 => Self::Daylight,
            0x200 => Self::Cloudy,
            0x300 => Self::FluorescentDaylight,
            0x301 | 0x303 | 0x304 => Self::FluorescentWarmWhite,
            0x302 => Self::FluorescentCoolWhite,
            0x400 => Self::Incandescent,
            0x500 => Self::Flash,
            0x600 => Self::Underwater,
            0xf00..=0xf04 => Self::Custom((v - 0xf00 + 1) as u8),
            0xff0 => Self::Kelvin,
            other => Self::Other(other),
        }
    }
}

/// Red/blue white-balance shift. Values are stored as raw maker-note values;
/// current X-series bodies store 20 per camera step (R+2 is stored as 40).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct WbShift {
    pub red_raw: i32,
    pub blue_raw: i32,
}

impl WbShift {
    /// Shift in camera menu steps (-9..=+9), when the raw values use the
    /// ×20 scale of current bodies.
    pub fn steps(&self) -> Option<(i32, i32)> {
        (self.red_raw % 20 == 0 && self.blue_raw % 20 == 0)
            .then_some((self.red_raw / 20, self.blue_raw / 20))
    }
}

/// Dynamic range setting: automatic, or a fixed DR100 / DR200 / DR400.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum DynamicRange {
    Auto,
    Percent(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum DRangePriority {
    /// Automatic D-Range Priority, with the strength the camera chose when recorded.
    Auto(Option<DRangeStrength>),
    Fixed(DRangeStrength),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum DRangeStrength {
    Weak,
    Strong,
    Plus,
}

/// The in-camera settings that make up a Fujifilm "recipe".
///
/// Every field is optional: older bodies don't record some of them, and a
/// value outside the known codes is left out rather than guessed.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct FujiSettings {
    pub film_simulation: Option<FilmSimulation>,
    pub grain_roughness: Option<Strength>,
    pub grain_size: Option<GrainSize>,
    pub color_chrome: Option<Strength>,
    pub color_chrome_fx_blue: Option<Strength>,
    /// Camera scale, e.g. `+1.0`, `-2.0`, `+0.5`.
    pub shadow_tone: Option<f32>,
    pub highlight_tone: Option<f32>,
    /// "Color" (saturation) in camera steps, -4..=+4. Absent for monochrome.
    pub color: Option<i8>,
    pub sharpness: Option<i8>,
    pub noise_reduction: Option<i8>,
    pub clarity: Option<i8>,
    pub white_balance: Option<WhiteBalance>,
    pub color_temperature_k: Option<u16>,
    pub wb_shift: Option<WbShift>,
    pub dynamic_range: Option<DynamicRange>,
    pub d_range_priority: Option<DRangePriority>,
    /// Monochromatic colour: warm (+) / cool (−).
    pub mono_warm_cool: Option<i8>,
    /// Monochromatic colour: green (+) / magenta (−).
    pub mono_magenta_green: Option<i8>,
}

fn color_steps(code: u16) -> Option<i8> {
    Some(match code {
        0x0 => 0,
        0x080 => 1,
        0x100 => 2,
        0x0c0 => 3,
        0x0e0 => 4,
        0x180 => -1,
        0x400 => -2,
        0x4c0 => -3,
        0x4e0 => -4,
        _ => return None,
    })
}

fn sharpness_steps(code: u16) -> Option<i8> {
    Some(match code {
        0x00 => -4,
        0x01 => -3,
        0x02 => -2,
        0x82 => -1,
        0x03 => 0,
        0x84 => 1,
        0x04 => 2,
        0x05 => 3,
        0x06 => 4,
        _ => return None,
    })
}

fn nr_steps(code: u16) -> Option<i8> {
    Some(match code {
        0x000 => 0,
        0x180 => 1,
        0x100 => 2,
        0x1c0 => 3,
        0x1e0 => 4,
        0x280 => -1,
        0x200 => -2,
        0x2c0 => -3,
        0x2e0 => -4,
        _ => return None,
    })
}

/// Tone curve values are stored as −16 per camera step (so −32 means +2).
fn tone(v: i64) -> f32 {
    // Negate as an integer first so 0 stays 0.0, not -0.0.
    (v.saturating_neg() as f32) / 16.0
}

/// Parses a Fujifilm maker note. `data` is the buffer holding it and `start`
/// its absolute position. Returns `None` when the note is not a Fujifilm one.
pub(crate) fn parse(data: &[u8], start: usize) -> Option<FujiSettings> {
    let b = Bytes::new(data, Endian::Little);
    if b.slice(start, 8).ok()? != b"FUJIFILM" {
        return None;
    }
    let ifd_off = b.u32(start + 8).ok()? as usize;
    let ifd = Ifd::read(b, start, ifd_off).ok()?;
    let u16_of = |t| ifd.int(t).and_then(|v| u16::try_from(v).ok());

    let saturation = u16_of(tag::SATURATION);
    let film = FilmSimulation::from_codes(u16_of(tag::FILM_MODE), saturation);

    let wb_shift = ifd.ints(tag::WB_FINE_TUNE, 2).and_then(|v| match v[..] {
        [r, b] => Some(WbShift {
            red_raw: r as i32,
            blue_raw: b as i32,
        }),
        _ => None,
    });

    let dynamic_range = match u16_of(tag::DYNAMIC_RANGE_SETTING) {
        Some(0x000) => Some(DynamicRange::Auto),
        Some(0x001) => u16_of(tag::DEVELOPMENT_DYNAMIC_RANGE).map(DynamicRange::Percent),
        Some(0x100) => Some(DynamicRange::Percent(100)),
        Some(0x200) => Some(DynamicRange::Percent(200)),
        Some(0x201) => Some(DynamicRange::Percent(400)),
        _ => None,
    };

    let strength = |v| match v {
        Some(1) => Some(DRangeStrength::Weak),
        Some(2) => Some(DRangeStrength::Strong),
        Some(3) => Some(DRangeStrength::Plus),
        _ => None,
    };
    let d_range_priority = match ifd.int(tag::D_RANGE_PRIORITY) {
        Some(0) => Some(DRangePriority::Auto(strength(
            ifd.int(tag::D_RANGE_PRIORITY_AUTO),
        ))),
        Some(1) => strength(ifd.int(tag::D_RANGE_PRIORITY_FIXED)).map(DRangePriority::Fixed),
        _ => None,
    };

    let mono = film.is_some_and(|f| f.is_monochrome());
    Some(FujiSettings {
        film_simulation: film,
        grain_roughness: ifd.int(tag::GRAIN_ROUGHNESS).and_then(Strength::from_code),
        grain_size: match ifd.int(tag::GRAIN_SIZE) {
            Some(0) => Some(GrainSize::Off),
            Some(16) => Some(GrainSize::Small),
            Some(32) => Some(GrainSize::Large),
            _ => None,
        },
        color_chrome: ifd.int(tag::COLOR_CHROME).and_then(Strength::from_code),
        color_chrome_fx_blue: ifd
            .int(tag::COLOR_CHROME_FX_BLUE)
            .and_then(Strength::from_code),
        shadow_tone: ifd.int(tag::SHADOW_TONE).map(tone),
        highlight_tone: ifd.int(tag::HIGHLIGHT_TONE).map(tone),
        color: if mono {
            None
        } else {
            saturation.and_then(color_steps)
        },
        sharpness: u16_of(tag::SHARPNESS).and_then(sharpness_steps),
        noise_reduction: u16_of(tag::NOISE_REDUCTION).and_then(nr_steps),
        clarity: ifd
            .int(tag::CLARITY)
            .filter(|v| v % 1000 == 0 && v.abs() <= 5000)
            .map(|v| (v / 1000) as i8),
        white_balance: u16_of(tag::WHITE_BALANCE).map(WhiteBalance::from_code),
        color_temperature_k: u16_of(tag::COLOR_TEMPERATURE).filter(|&k| k > 0),
        wb_shift,
        dynamic_range,
        d_range_priority,
        mono_warm_cool: if mono {
            ifd.first_i8(tag::BW_ADJUSTMENT)
        } else {
            None
        },
        mono_magenta_green: if mono {
            ifd.first_i8(tag::BW_MAGENTA_GREEN)
        } else {
            None
        },
    })
}
