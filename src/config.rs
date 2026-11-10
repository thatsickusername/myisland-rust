#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IslandState {
    Rest,
    Compact,
    Expanded,
    Tucked,
}

#[derive(Debug, Clone, Copy)]
pub struct IslandDims {
    pub w: f32,
    pub h: f32,
    pub r: f32,
}

pub const WINDOW_WIDTH: i32 = 860;
pub const WINDOW_HEIGHT: i32 = 340;

pub const DIMS_REST: IslandDims = IslandDims { w: 116.0, h: 32.0, r: 16.0 };
pub const DIMS_COMPACT: IslandDims = IslandDims { w: 300.0, h: 36.0, r: 18.0 };
pub const DIMS_EXPANDED: IslandDims = IslandDims { w: 520.0, h: 170.0, r: 30.0 };
pub const DIMS_TUCKED: IslandDims = IslandDims { w: 56.0, h: 4.0, r: 2.0 };
pub const DIMS_SECONDARY: IslandDims = IslandDims { w: 44.0, h: 36.0, r: 18.0 };

pub fn notch_scale(size_idx: usize) -> f32 {
    match size_idx {
        0 => 0.85, // Small (85%)
        1 => 1.0,  // Medium (100% - Default)
        2 => 1.15, // Large (115%)
        _ => 1.0,
    }
}

pub fn scaled_dims(base: IslandDims, scale: f32) -> IslandDims {
    IslandDims {
        w: (base.w * scale).round(),
        h: (base.h * scale).round(),
        r: (base.r * scale).round(),
    }
}

// Compact music pill width range when synced lyrics are visible.
pub const COMPACT_LYRICS_MIN_W: f32 = 300.0;
pub const COMPACT_LYRICS_MAX_W: f32 = 500.0;

pub const SECONDARY_GAP: f32 = 28.0;

// Mascot colors
#[allow(dead_code)]
pub const COLOR_BASE_MINT: &str = "#e3ffd4";
#[allow(dead_code)]
pub const COLOR_GREEN: &str = "#30d158";
#[allow(dead_code)]
pub const COLOR_RED: &str = "#ff453a";
#[allow(dead_code)]
pub const COLOR_GOLD: &str = "#ffd60a";
#[allow(dead_code)]
pub const COLOR_BLUE: &str = "#64d2ff";
#[allow(dead_code)]
pub const COLOR_PINK: &str = "#ff3388";
#[allow(dead_code)]
pub const COLOR_ORANGE: &str = "#f0a064";
