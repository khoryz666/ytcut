//! Rendering: every `AppUi` method that draws to the frame.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::downloads::Status;
use crate::update::UPDATE_NOTICE;

use super::palette::{matching_commands, COMMAND_COL_WIDTH};
use super::text::{crop_word, truncate_str, wrap_word};
use super::theme;
use super::widgets::{
    centered_rect, field_label, field_value, help_header, help_row, panel, progress_bar,
    spaced_offset, toggle_button, volume_bar, with_gaps,
};
use super::{AddField, AppUi, Focus, PromptType, SettingsRow};

impl AppUi {
    pub(super) fn render(&self, f: &mut ratatui::Frame) {
        let size = f.area();

        // A single base surface so the whole screen reads as one app.
        f.render_widget(Block::default().style(Style::default().bg(theme::BG)), size);

        let has_running_job = {
            let jobs = self.jobs.lock().unwrap();
            jobs.iter().any(|j| j.status == Status::Running)
        };

        let mut constraints: Vec<Constraint> = Vec::new();
        if has_running_job {
            constraints.push(Constraint::Length(1)); // download banner
        }
        constraints.push(Constraint::Length(1)); // header
        constraints.push(Constraint::Min(8)); // library & queue
        constraints.push(Constraint::Length(3)); // player bar
        constraints.push(Constraint::Length(1)); // status bar

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .spacing(1)
            .split(size);

        let mut idx = 0;
        let banner = if has_running_job {
            let area = chunks[idx];
            idx += 1;
            Some(area)
        } else {
            None
        };
        let header = chunks[idx];
        idx += 1;
        let middle = chunks[idx];
        idx += 1;
        let player = chunks[idx];
        idx += 1;
        let status = chunks[idx];

        if let Some(banner) = banner {
            self.render_download_banner(f, banner);
        }
        self.render_header(f, header);
        self.render_middle_columns(f, middle);
        self.render_player_bar(f, player);
        self.render_bottom_status(f, status);

        if let Some(prompt) = &self.prompt {
            self.render_prompt(f, prompt, size);
        }
        if self.focus == Focus::Add {
            self.render_add_dialog(f, size);
        }
        if self.focus == Focus::Command {
            self.render_command_palette(f, size);
        }
        if self.focus == Focus::Settings {
            self.render_settings(f, size);
        }
        if self.focus == Focus::Help {
            self.render_help(f, size);
        }
    }

    pub(super) fn render_header(&self, f: &mut ratatui::Frame, area: Rect) {
        let version = env!("CARGO_PKG_VERSION");
        let count = self.songs.len();
        let songs = if count == 1 { "song" } else { "songs" };
        let dir = truncate_str(&self.library_dir.display().to_string(), 40);

        let wordmark = " ytcut";
        let version = format!(" v{version} ");
        let right = format!("{count} {songs} · {dir}  ");

        let width = area.width as usize;
        let left_w = wordmark.chars().count() + version.chars().count();
        let right_w = right.chars().count();
        let pad = width.saturating_sub(left_w + right_w);

        let line = Line::from(vec![
            Span::styled(
                wordmark,
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(version, Style::default().fg(theme::MUTED)),
            Span::raw(" ".repeat(pad)),
            Span::styled(right, Style::default().fg(theme::MUTED)),
        ]);

        f.render_widget(
            Paragraph::new(line).style(Style::default().bg(theme::BG)),
            area,
        );
    }

    pub(super) fn render_download_banner(&self, f: &mut ratatui::Frame, area: Rect) {
        let jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.iter().rev().find(|j| j.status == Status::Running) {
            let label = truncate_str(&job.label, 26);
            let bar_w = area.width.saturating_sub(22).max(8);

            let mut spans: Vec<Span> = vec![
                Span::styled(
                    " ↓ ",
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{label} "), Style::default().fg(theme::FG)),
            ];
            spans.extend(progress_bar(job.progress / 100.0, bar_w));
            spans.push(Span::styled(
                format!(" {:.0}%", job.progress),
                Style::default().fg(theme::MUTED),
            ));

            f.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::BG)),
                area,
            );
        }
    }

    pub(super) fn render_add_dialog(&self, f: &mut ratatui::Frame, area: Rect) {
        let popup = centered_rect(68, 52, area);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Add from YouTube ")
            .title_style(
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme::ACCENT))
            .style(Style::default().bg(theme::SURFACE));
        f.render_widget(block, popup);

        let inner = popup.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 2,
        });

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // URL
                Constraint::Length(2), // Name
                Constraint::Length(2), // Cut
                Constraint::Min(1),    // spacer
                Constraint::Length(1), // footer
            ])
            .split(inner);

        let url_focused = self.add_field == AddField::Url;
        let url_line = Line::from(vec![
            field_label("URL", url_focused),
            field_value(&self.add_url, "<paste or type a YouTube URL>", url_focused),
        ]);
        f.render_widget(Paragraph::new(url_line), rows[0]);

        let name_focused = self.add_field == AddField::Name;
        let name_line = Line::from(vec![
            field_label("Name", name_focused),
            field_value(
                &self.add_name,
                "<optional — defaults to the video title>",
                name_focused,
            ),
        ]);
        f.render_widget(Paragraph::new(name_line), rows[1]);

        let start_focused = self.add_field == AddField::Start;
        let end_focused = self.add_field == AddField::End;
        let cut_active = start_focused || end_focused;
        let cut_line = Line::from(vec![
            field_label("Cut", cut_active),
            Span::styled("Start ", Style::default().fg(theme::MUTED)),
            field_value(&self.add_start, "0:00", start_focused),
            Span::styled("  End ", Style::default().fg(theme::MUTED)),
            field_value(&self.add_end, "end", end_focused),
        ]);
        f.render_widget(Paragraph::new(cut_line), rows[2]);

        f.render_widget(
            Paragraph::new(Span::styled(
                "Tab · next field      Enter · download      Esc · cancel",
                Style::default().fg(theme::MUTED),
            )),
            rows[4],
        );
    }

    pub(super) fn render_middle_columns(&self, f: &mut ratatui::Frame, area: Rect) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .spacing(1)
            .split(area);

        self.render_library_pane(f, cols[0]);
        self.render_queue_pane(f, cols[1]);
    }

    pub(super) fn render_library_pane(&self, f: &mut ratatui::Frame, area: Rect) {
        let focused = self.focus == Focus::Library;
        let title = if self.search_query.is_empty() {
            " Library ".to_string()
        } else {
            format!(" Library · “{}” ", truncate_str(&self.search_query, 20))
        };
        let block = panel(title, focused);

        if self.filtered_indices.is_empty() {
            let hint = if self.songs.is_empty() {
                "No songs yet — run /add <url> to download one"
            } else {
                "No songs match your search"
            };
            f.render_widget(
                Paragraph::new(hint)
                    .style(Style::default().fg(theme::MUTED))
                    .block(block),
                area,
            );
            return;
        }

        let name_max = area.width.saturating_sub(8) as usize;
        let mut items: Vec<ListItem> = Vec::with_capacity(self.filtered_indices.len() * 2);
        let mut starts: Vec<usize> = Vec::with_capacity(self.filtered_indices.len());
        let mut heights: Vec<usize> = Vec::with_capacity(self.filtered_indices.len());

        for (display_idx, &song_idx) in self.filtered_indices.iter().enumerate() {
            let song = &self.songs[song_idx];
            let selected = display_idx == self.lib_selected && focused;

            starts.push(items.len());
            let wrapped = wrap_word(&song.name, name_max);
            heights.push(wrapped.len());

            let marker = if selected { "▶ " } else { "  " };
            let text_style = if selected {
                Style::default().fg(theme::BG).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::FG)
            };
            let item_style = if selected {
                Style::default().bg(theme::ACCENT)
            } else {
                Style::default()
            };

            for (i, line) in wrapped.iter().enumerate() {
                let prefix = if i == 0 {
                    format!("  {marker}")
                } else {
                    "    ".to_string()
                };
                items.push(
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, text_style),
                        Span::styled(line.clone(), text_style),
                    ]))
                    .style(item_style),
                );
            }
            items.push(ListItem::new(""));
        }

        let list = List::new(items).block(block);
        let mut state = ListState::default();
        if let (Some(&start), Some(&height)) = (
            starts.get(self.lib_selected),
            heights.get(self.lib_selected),
        ) {
            let visible = area.height.saturating_sub(2) as usize;
            let offset = if height >= visible {
                start
            } else {
                start.saturating_sub(visible.saturating_sub(height))
            };
            *state.offset_mut() = offset;
        }
        f.render_stateful_widget(list, area, &mut state);
    }

    pub(super) fn render_queue_pane(&self, f: &mut ratatui::Frame, area: Rect) {
        let focused = self.focus == Focus::Playlist;
        let count = self.player.as_ref().map(|p| p.queue.len()).unwrap_or(0);
        let block = panel(format!(" Queue ({count}) "), focused);

        let Some(player) = &self.player else {
            f.render_widget(
                Paragraph::new("No audio device available")
                    .style(Style::default().fg(theme::RED))
                    .block(block),
                area,
            );
            return;
        };

        if player.queue.is_empty() {
            f.render_widget(
                Paragraph::new("Queue is empty — play a song to fill it")
                    .style(Style::default().fg(theme::MUTED))
                    .block(block),
                area,
            );
            return;
        }

        let current_idx = player.current_song_index();
        let items: Vec<ListItem> = player
            .queue
            .iter()
            .enumerate()
            .map(|(idx, path)| {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let is_current = current_idx == Some(idx);
                let is_selected = idx == self.playlist_selected && focused;

                let marker = if is_current { "▶ " } else { "  " };
                let num = format!("{:>2} ", idx + 1);
                let name = crop_word(&name, area.width.saturating_sub(8) as usize);

                let text_style = if is_selected {
                    Style::default().fg(theme::BG).add_modifier(Modifier::BOLD)
                } else if is_current {
                    Style::default()
                        .fg(theme::GREEN)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::FG)
                };
                let item_style = if is_selected {
                    Style::default().bg(theme::ACCENT)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {marker}{num}"), text_style),
                    Span::styled(name, text_style),
                ]))
                .style(item_style)
            })
            .collect();

        let list = List::new(with_gaps(items)).block(block);
        let mut state = ListState::default();
        *state.offset_mut() = spaced_offset(self.playlist_selected, area.height);
        f.render_stateful_widget(list, area, &mut state);
    }

    pub(super) fn render_player_bar(&self, f: &mut ratatui::Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::BORDER))
            .style(Style::default().bg(theme::SURFACE));
        f.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 2,
        });

        let Some(player) = &self.player else {
            f.render_widget(
                Paragraph::new("No audio device found — playback unavailable")
                    .style(Style::default().fg(theme::RED)),
                inner,
            );
            return;
        };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(3),      // play button
                Constraint::Length(1),      // gap
                Constraint::Percentage(24), // title
                Constraint::Min(6),         // progress bar
                Constraint::Length(1),      // gap
                Constraint::Length(13),     // time
                Constraint::Length(1),      // gap
                Constraint::Length(3),      // shuffle button
                Constraint::Length(1),      // gap
                Constraint::Length(3),      // repeat button
            ])
            .split(inner);

        let icon = if player.is_paused() {
            "‖"
        } else if player.is_started {
            "▶"
        } else {
            "■"
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                format!(" {icon} "),
                Style::default()
                    .fg(theme::BG)
                    .bg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )])),
            chunks[0],
        );

        let title = player
            .current_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Nothing playing".to_string());
        f.render_widget(
            Paragraph::new(truncate_str(&title, chunks[2].width as usize))
                .style(Style::default().fg(theme::FG).add_modifier(Modifier::BOLD)),
            chunks[2],
        );

        f.render_widget(
            Paragraph::new(Line::from(progress_bar(
                player.progress_ratio(),
                chunks[3].width,
            ))),
            chunks[3],
        );

        f.render_widget(
            Paragraph::new(player.progress_str())
                .alignment(Alignment::Right)
                .style(Style::default().fg(theme::MUTED)),
            chunks[5],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![toggle_button("⇆", player.shuffle)])),
            chunks[7],
        );
        f.render_widget(
            Paragraph::new(Line::from(vec![toggle_button("↻", player.repeat_one)])),
            chunks[9],
        );
    }

    pub(super) fn render_bottom_status(&self, f: &mut ratatui::Frame, area: Rect) {
        let mut spans: Vec<Span> = Vec::new();

        if let Some((msg, _)) = &self.status_message {
            spans.push(Span::styled(
                format!(" {msg} "),
                Style::default().fg(theme::YELLOW),
            ));
        } else if let Ok(guard) = UPDATE_NOTICE.lock() {
            if let Some(notice) = guard.as_ref() {
                spans.push(Span::styled(
                    format!(" ★ {notice} "),
                    Style::default()
                        .fg(theme::MAGENTA)
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }

        // Check if any job failed requiring sign in
        let jobs = self.jobs.lock().unwrap();
        if jobs
            .iter()
            .any(|j| j.status == Status::Error && j.message.contains("Sign in"))
        {
            spans.push(Span::styled(
                " [Sign in needed: press o to open YouTube] ",
                Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
            ));
        }

        if spans.is_empty() {
            spans.push(Span::styled(
                " Tab pane · space pause · / commands · ? help ",
                Style::default().fg(theme::MUTED),
            ));
        }

        f.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::SURFACE)),
            area,
        );
    }

    pub(super) fn render_command_palette(&self, f: &mut ratatui::Frame, area: Rect) {
        let matching = matching_commands(&self.command_input);
        let count = matching.len().max(1);
        let height = ((count as u16) + 4)
            .min(12)
            .min(area.height.saturating_sub(4));
        let width = 74.min(area.width.saturating_sub(4));

        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + 2,
            width,
            height,
        };
        f.render_widget(Clear, popup);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Command ")
            .title_style(
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme::ACCENT))
            .style(Style::default().bg(theme::SURFACE));
        f.render_widget(block, popup);

        let inner = popup.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // input line
                Constraint::Length(1), // separator
                Constraint::Min(1),    // suggestions
            ])
            .split(inner);

        let clean = self
            .command_input
            .strip_prefix('/')
            .unwrap_or(&self.command_input);
        f.render_widget(
            Paragraph::new(format!(" /{clean}"))
                .style(Style::default().fg(theme::FG).add_modifier(Modifier::BOLD)),
            chunks[0],
        );

        let sep = "─".repeat(chunks[1].width as usize);
        f.render_widget(
            Paragraph::new(sep).style(Style::default().fg(theme::BORDER)),
            chunks[1],
        );

        let items: Vec<ListItem> = if matching.is_empty() {
            vec![ListItem::new(Span::styled(
                format!("  No matching commands for \"/{clean}\""),
                Style::default().fg(theme::MUTED),
            ))]
        } else {
            matching
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let is_selected = idx == self.command_palette_selected;
                    let marker = if is_selected { "▶ " } else { "  " };

                    let (cmd_style, alias_style, desc_style) = if is_selected {
                        (
                            Style::default().fg(theme::BG).add_modifier(Modifier::BOLD),
                            Style::default().fg(theme::BG),
                            Style::default().fg(theme::BG),
                        )
                    } else {
                        (
                            Style::default()
                                .fg(theme::ACCENT)
                                .add_modifier(Modifier::BOLD),
                            Style::default().fg(theme::MUTED),
                            Style::default().fg(theme::MUTED),
                        )
                    };
                    let item_style = if is_selected {
                        Style::default().bg(theme::ACCENT)
                    } else {
                        Style::default()
                    };

                    let alias = if item.alias.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", item.alias)
                    };
                    let left_w =
                        marker.chars().count() + item.cmd.chars().count() + alias.chars().count();
                    let pad = " ".repeat(COMMAND_COL_WIDTH.saturating_sub(left_w));

                    let line = Line::from(vec![
                        Span::styled(format!("{marker}{}", item.cmd), cmd_style),
                        Span::styled(alias, alias_style),
                        Span::raw(pad),
                        Span::styled(item.desc, desc_style),
                    ]);
                    ListItem::new(line).style(item_style)
                })
                .collect()
        };

        f.render_widget(List::new(items), chunks[2]);
    }

    pub(super) fn dir_display(&self) -> String {
        self.library_dir.display().to_string()
    }

    pub(super) fn settings_line(
        &self,
        row: SettingsRow,
        label: &str,
        value: String,
    ) -> ListItem<'static> {
        let selected = self.settings.selected == row;
        let marker = if selected { "▶ " } else { "  " };
        let text = if row == SettingsRow::Dir && self.settings.editing_dir {
            format!("{marker}{label}: {}", self.settings.dir_buf)
        } else {
            format!("{marker}{label}: {value}")
        };
        let fg = if selected {
            Style::default().fg(theme::BG).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::FG)
        };
        let bg = if selected {
            Style::default().bg(theme::ACCENT)
        } else {
            Style::default()
        };
        ListItem::new(Span::styled(text, fg)).style(bg)
    }

    pub(super) fn render_settings(&self, f: &mut ratatui::Frame, area: Rect) {
        let popup = centered_rect(72, 55, area);
        f.render_widget(Clear, popup);

        let rows = vec![
            self.settings_line(SettingsRow::Dir, "Library folder", self.dir_display()),
            self.settings_line(
                SettingsRow::Browser,
                "Sign-in browser",
                self.config.browser.clone().unwrap_or_else(|| "auto".into()),
            ),
            self.settings_line(
                SettingsRow::Autoupdate,
                "Auto-update",
                self.config
                    .autoupdate
                    .clone()
                    .unwrap_or_else(|| "auto".into()),
            ),
            self.settings_line(
                SettingsRow::Volume,
                "Volume",
                volume_bar(self.config.volume()),
            ),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Settings ")
            .title_style(
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme::ACCENT))
            .style(Style::default().bg(theme::SURFACE));
        f.render_widget(block, popup);

        let inner = popup.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 2,
        });
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(4), Constraint::Length(1)])
            .split(inner);
        f.render_widget(List::new(rows), chunks[0]);
        f.render_widget(
            Paragraph::new(Span::styled(
                "Enter · edit / cycle      ← → · change value      Esc · close",
                Style::default().fg(theme::MUTED),
            )),
            chunks[1],
        );
    }

    pub(super) fn render_help(&self, f: &mut ratatui::Frame, area: Rect) {
        let popup = centered_rect(76, 90, area);
        f.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Help ")
            .title_style(
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme::ACCENT))
            .style(Style::default().bg(theme::SURFACE));

        let lines = vec![
            Line::from(""),
            help_header("Navigation"),
            Line::from(""),
            help_row("Tab", "switch pane / field"),
            help_row("j / ↓", "move down"),
            help_row("k / ↑", "move up"),
            help_row("Enter", "play selection"),
            help_row("d", "delete song"),
            help_row("/", "command palette"),
            help_row("?", "this help"),
            Line::from(""),
            help_header("Playback"),
            Line::from(""),
            help_row("space", "play / pause"),
            help_row("n", "next track"),
            help_row("p", "previous track"),
            help_row("s", "toggle shuffle"),
            help_row("r", "toggle repeat one"),
            help_row("o", "open YouTube (sign in)"),
            Line::from(""),
            help_header("Commands"),
            Line::from(""),
            help_row("/add", "download a song"),
            help_row("/background", "play in background (/bg)"),
            help_row("/stop", "stop background audio"),
            help_row("/search", "filter the library"),
            help_row("/refresh", "reload the library"),
            help_row("/settings", "open settings"),
            help_row("/dir", "change library folder"),
            help_row("/update", "update ytcut"),
            help_row("/quit", "exit"),
            Line::from(""),
        ];

        f.render_widget(List::new(lines).block(block), popup);
    }

    pub(super) fn render_prompt(&self, f: &mut ratatui::Frame, prompt: &PromptType, area: Rect) {
        let popup_area = centered_rect(60, 20, area);
        f.render_widget(Clear, popup_area);

        match prompt {
            PromptType::Delete(name) => {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Delete ")
                    .title_style(Style::default().fg(theme::RED).add_modifier(Modifier::BOLD))
                    .border_style(Style::default().fg(theme::RED))
                    .style(Style::default().bg(theme::SURFACE));
                let text = format!("\n  Delete “{name}”?\n\n  [y] confirm    [n] cancel");
                let p = Paragraph::new(text).block(block);
                f.render_widget(p, popup_area);
            }
        }
    }
}
