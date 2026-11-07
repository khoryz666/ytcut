//! The ytcut color palette: a cool, dark theme shared across every widget.

use ratatui::style::Color;

/// Near-black base with a cool blue tint.
pub const BG: Color = Color::Rgb(15, 16, 25);
/// Panel and popup background.
pub const SURFACE: Color = Color::Rgb(22, 24, 38);
/// Slightly raised surface, e.g. inactive button chips.
pub const SURFACE_ALT: Color = Color::Rgb(31, 34, 54);
/// Inactive panel borders.
pub const BORDER: Color = Color::Rgb(52, 58, 88);
/// Primary text.
pub const FG: Color = Color::Rgb(201, 207, 233);
/// Dim text, hints, file sizes.
pub const MUTED: Color = Color::Rgb(96, 105, 150);
/// Accent: focus, selection, progress.
pub const ACCENT: Color = Color::Rgb(122, 162, 247);
/// Playing / success.
pub const GREEN: Color = Color::Rgb(158, 206, 106);
/// Status messages.
pub const YELLOW: Color = Color::Rgb(224, 175, 104);
/// Errors and destructive actions.
pub const RED: Color = Color::Rgb(247, 118, 142);
/// Update notices.
pub const MAGENTA: Color = Color::Rgb(187, 154, 247);
