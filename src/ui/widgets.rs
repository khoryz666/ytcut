//! Reusable ratatui widgets and layout helpers.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, ListItem};

use super::theme;

/// A volume bar like `██████░░░░  60%`.
pub fn volume_bar(volume: f32) -> String {
    let v = volume.clamp(0.0, 1.0);
    let filled = (v * 10.0).round() as usize;
    let bar: String = (0..10)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect();
    format!("{bar} {:>3}%", (v * 100.0).round() as i32)
}

/// A rounded, themed panel with an accent border when focused.
pub fn panel(title: String, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .title_style(
            Style::default()
                .fg(if focused { theme::ACCENT } else { theme::MUTED })
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(if focused {
            theme::ACCENT
        } else {
            theme::BORDER
        }))
        .style(Style::default().bg(theme::SURFACE))
}

/// A filled/unfilled progress bar with a knob, as styled spans.
pub fn progress_bar(ratio: f64, width: u16) -> Vec<Span<'static>> {
    let ratio = ratio.clamp(0.0, 1.0);
    let width = width as usize;
    if width == 0 {
        return Vec::new();
    }
    let cells = width.saturating_sub(1);
    let filled = ((ratio * cells as f64).round() as usize).min(cells);

    let mut spans: Vec<Span> = Vec::new();
    if filled > 0 {
        spans.push(Span::styled(
            "█".repeat(filled),
            Style::default().fg(theme::ACCENT),
        ));
    }
    spans.push(Span::styled("●", Style::default().fg(theme::FG)));
    let empty = cells.saturating_sub(filled);
    if empty > 0 {
        spans.push(Span::styled(
            "░".repeat(empty),
            Style::default().fg(theme::MUTED),
        ));
    }
    spans
}

/// Keep the selected row visible: offset so `selected` sits within `visible` rows.
pub fn scroll_offset(selected: usize, visible: usize) -> usize {
    let visible = visible.saturating_sub(1).max(1);
    selected.saturating_sub(visible)
}

/// A form-field label, accent and arrowed when focused.
pub fn field_label(name: &str, focused: bool) -> Span<'static> {
    let text = if focused {
        format!("▶ {name:<5}")
    } else {
        format!("  {name:<5}")
    };
    Span::styled(
        text,
        if focused {
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::MUTED)
        },
    )
}

/// A form-field value: a dim placeholder when empty, underlined while focused.
pub fn field_value(value: &str, placeholder: &str, focused: bool) -> Span<'static> {
    if value.is_empty() {
        Span::styled(
            placeholder.to_string(),
            Style::default()
                .fg(theme::MUTED)
                .add_modifier(Modifier::DIM),
        )
    } else if focused {
        Span::styled(
            value.to_string(),
            Style::default()
                .fg(theme::FG)
                .add_modifier(Modifier::UNDERLINED),
        )
    } else {
        Span::styled(value.to_string(), Style::default().fg(theme::FG))
    }
}

/// Insert a blank line after each item so rows are taller and easier to scan.
pub fn with_gaps(items: Vec<ListItem<'static>>) -> Vec<ListItem<'static>> {
    let mut out = Vec::with_capacity(items.len().saturating_mul(2));
    for item in items {
        out.push(item);
        out.push(ListItem::new(""));
    }
    out
}

/// A bold, accent-colored section heading for the help page.
pub fn help_header(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    ))
}

/// A help row: a bold accent key column followed by its description.
pub fn help_row(key: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<14}", key),
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc.to_string(), Style::default().fg(theme::FG)),
    ])
}

/// A padded "button" chip for the player bar, lit green when active.
pub fn toggle_button(symbol: &str, on: bool) -> Span<'static> {
    let style = if on {
        Style::default()
            .fg(theme::BG)
            .bg(theme::GREEN)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::MUTED).bg(theme::SURFACE_ALT)
    };
    Span::styled(format!(" {symbol} "), style)
}

/// Scroll offset for a gap-spaced list (two buffer rows per song).
pub fn spaced_offset(selected: usize, area_height: u16) -> usize {
    let visible = ((area_height.saturating_sub(2)) as usize).div_ceil(2);
    scroll_offset(selected, visible) * 2
}

/// A rectangle centered within `r`, sized as a percentage of it.
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
