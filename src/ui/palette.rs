//! The command palette: the list of commands and their fuzzy matching.

/// A single entry in the command palette.
#[derive(Clone, Copy, Debug)]
pub struct PaletteItem {
    pub cmd: &'static str,
    pub alias: &'static str,
    pub desc: &'static str,
}

/// Fixed width of the command column in the palette so descriptions line up.
pub const COMMAND_COL_WIDTH: usize = 22;

pub const COMMAND_PALETTE_ITEMS: &[PaletteItem] = &[
    PaletteItem {
        cmd: "/add",
        alias: "",
        desc: "Download a song",
    },
    PaletteItem {
        cmd: "/background",
        alias: "/bg",
        desc: "Play in background",
    },
    PaletteItem {
        cmd: "/stop",
        alias: "",
        desc: "Stop background audio",
    },
    PaletteItem {
        cmd: "/search",
        alias: "",
        desc: "Filter the library",
    },
    PaletteItem {
        cmd: "/refresh",
        alias: "",
        desc: "Reload the library",
    },
    PaletteItem {
        cmd: "/settings",
        alias: "",
        desc: "Open settings",
    },
    PaletteItem {
        cmd: "/dir",
        alias: "",
        desc: "Change library folder",
    },
    PaletteItem {
        cmd: "/update",
        alias: "",
        desc: "Update ytcut",
    },
    PaletteItem {
        cmd: "/help",
        alias: "",
        desc: "Show help",
    },
    PaletteItem {
        cmd: "/quit",
        alias: "",
        desc: "Quit",
    },
];

/// Commands matching `input`, by substring of the command, alias, or description.
pub fn matching_commands(input: &str) -> Vec<&'static PaletteItem> {
    let clean = input
        .trim()
        .strip_prefix('/')
        .unwrap_or(input.trim())
        .to_lowercase();
    let query_cmd = clean.split_whitespace().next().unwrap_or_default();

    if query_cmd.is_empty() {
        return COMMAND_PALETTE_ITEMS.iter().collect();
    }

    COMMAND_PALETTE_ITEMS
        .iter()
        .filter(|item| {
            let item_clean = item.cmd.strip_prefix('/').unwrap_or(item.cmd);
            let alias_clean = item.alias.strip_prefix('/').unwrap_or(item.alias);
            item_clean.contains(query_cmd)
                || (!alias_clean.is_empty() && alias_clean.contains(query_cmd))
                || item.desc.to_lowercase().contains(query_cmd)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_commands_filters_correctly() {
        assert_eq!(matching_commands("").len(), COMMAND_PALETTE_ITEMS.len());
        assert_eq!(matching_commands("/").len(), COMMAND_PALETTE_ITEMS.len());
        assert_eq!(matching_commands("/set")[0].cmd, "/settings");
        assert_eq!(matching_commands("quit")[0].cmd, "/quit");
        assert_eq!(matching_commands("/add")[0].cmd, "/add");
        assert_eq!(matching_commands("/back")[0].cmd, "/background");
        assert_eq!(matching_commands("/bg")[0].cmd, "/background");
        assert_eq!(matching_commands("/stop")[0].cmd, "/stop");
        assert_eq!(matching_commands("/search")[0].cmd, "/search");
        assert_eq!(matching_commands("/refresh")[0].cmd, "/refresh");
    }
}
