//! Colours and sizes shared by every screen.

// Palette: dark charcoal with a single blue accent, matching the reference UI.
pub const BG: u32 = 0x1e2025;
pub const PANEL: u32 = 0x23262c;
pub const CHIP: u32 = 0x181a1f;
pub const BORDER: u32 = 0x343842;
pub const TEXT: u32 = 0xe4e7ec;
pub const MUTED: u32 = 0x8d95a3;
pub const SELECTED: u32 = 0x2c3e5c;
pub const HOVER: u32 = 0x2a2e36;
pub const ACCENT: u32 = 0x3f7fd9;
pub const SUCCESS: u32 = 0x5cc68a;
pub const AMBER: u32 = 0xe0a04a;
pub const ERROR: u32 = 0xe06c75;

pub const ROW_HEIGHT: f32 = 34.0;
pub const DETAILS_WIDTH: f32 = 300.0;
/// Grid view draws at most this many cards; the rest need a filter or list view.
pub const GRID_LIMIT: usize = 300;
