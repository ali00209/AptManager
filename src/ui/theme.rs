//! Colours and sizes shared by every screen. Values follow the Design canvas.

pub const BG: u32 = 0x111318;
pub const PANEL: u32 = 0x1a1d24;
pub const CHIP: u32 = 0x111318;
pub const BORDER: u32 = 0x2a2f3a;
pub const TEXT: u32 = 0xe6e8ee;
pub const TEXT_STRONG: u32 = 0xc3c8d6;
/// Lightened from the canvas's #8b93a7 so it passes 4.5:1 on hovered rows.
pub const MUTED: u32 = 0x9aa2b4;
pub const SELECTED: u32 = 0x161922;
pub const HOVER: u32 = 0x2a2f3a;
pub const ACCENT: u32 = 0x5b8def;
pub const SUCCESS: u32 = 0x3fb950;
pub const AMBER: u32 = 0xf2b35c;
/// Red for backgrounds and borders.
pub const ERROR: u32 = 0xe5534b;
/// Red for text: passes 4.5:1 on the dark panel.
pub const ERROR_TEXT: u32 = 0xf08a8a;
/// Scrim behind confirmation dialogs.
pub const SCRIM: u32 = 0x05070c99;

pub const ROW_HEIGHT: f32 = 52.0;
pub const DETAILS_WIDTH: f32 = 360.0;
/// Grid view draws at most this many cards; the rest need a filter or list view.
pub const GRID_LIMIT: usize = 300;
