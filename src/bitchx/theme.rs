//! # Themes
//!
//! `/theme` palettes: the built-in `bitchx` (the default, [`Palette::bitchx`])
//! and `terminal` ([`Palette::terminal`]: use your terminal's own colours),
//! the iamb-format `themes/*.toml` shipped in the source tree (embedded at
//! build time by [`BUILTIN`]) and any `*.toml` in `<config dir>/themes`.
//!
//! ## Mapping iamb theme roles onto the BitchX palette
//!
//! An iamb theme names its colours by role (see the header comment of every
//! `themes/*.toml`); they are read from the keys the file sets:
//!
//! - `COLOR_BG1` `theme.default.background`: screen background
//! - `COLOR_BG2` `theme.messages.code.background`: status bar background
//! - `COLOR_UI1` `theme.windows.border.color`: punctuation (`%K`), DBG text
//! - `COLOR_UI2` `theme.windows.title.color`: others' nick brackets,
//!   `/names` nicks, status bar brackets, the logo, logo blue ink
//! - `COLOR_WARN1` `theme.cmdbar.error.color`: errors, sent queries,
//!   `[SYNC LOST]`, logo red ink
//! - `COLOR_WARN2` `theme.timeline.redacted.color`: DBG tags
//! - `COLOR_WARN3` `theme.encryption.icon_unknown.color`: mentions /
//!   highlight (BitchX's yellow), status bar `Act:` mentions
//! - `COLOR_GOOD1` `theme.encryption.icon_encrypted.color`: BitchX's green
//!   (`[Users]`, whois ruler, `+` flag, `zZzZ`)
//! - `COLOR_TEXT1` `theme.default.color`: plain text, input line, bold
//!   status values
//! - `COLOR_TEXT2` `theme.messages.code.color`: BitchX's cyan (hosts,
//!   channels, joins, ops), logo cyan ink
//! - `COLOR_TEXT3` `theme.messages.strong.color`: own nick brackets,
//!   received queries and notices (BitchX's magenta), logo magenta ink
//! - `COLOR_TEXT4` `theme.messages.emphasis.color`: status bar text,
//!   timestamps, `/set` values
//! - `COLOR_TEXT5` `theme.rooms.default.color`: bright text (`%W`), logo
//!   white ink
//!
//! `theme.users.colors` is not used: BitchX does not colour nicks one by
//! one. A role whose key is missing keeps its `bitchx` colour; a key that is
//! not a colour makes the whole file invalid.
use std::path::Path;

use ratatui::style::Color;

use crate::bitchx::palette::Palette;

/// The name of the default theme.
pub(crate) const DEFAULT_THEME: &str = "bitchx";

/// The theme that overrides no colour at all ([`Palette::terminal`]).
pub(crate) const TERMINAL_THEME: &str = "terminal";

/// The upstream themes, embedded at build time (`themes/*.toml`).
pub(crate) const BUILTIN: &[(&str, &str)] = &[
    ("apprentice-dark", include_str!("../../themes/apprentice-dark.toml")),
    ("aurora-dark", include_str!("../../themes/aurora-dark.toml")),
    ("ayu-dark", include_str!("../../themes/ayu-dark.toml")),
    ("ayu-light", include_str!("../../themes/ayu-light.toml")),
    ("ayu-mirage-dark", include_str!("../../themes/ayu-mirage-dark.toml")),
    ("carbonfox-dark", include_str!("../../themes/carbonfox-dark.toml")),
    ("catppuccin-frappe-dark", include_str!("../../themes/catppuccin-frappe-dark.toml")),
    ("catppuccin-latte-dark", include_str!("../../themes/catppuccin-latte-dark.toml")),
    ("catppuccin-light", include_str!("../../themes/catppuccin-light.toml")),
    ("catppuccin-macchiato-dark", include_str!("../../themes/catppuccin-macchiato-dark.toml")),
    ("catppuccin-mocha-dark", include_str!("../../themes/catppuccin-mocha-dark.toml")),
    ("challenger-deep-dark", include_str!("../../themes/challenger-deep-dark.toml")),
    ("dawnfox-light", include_str!("../../themes/dawnfox-light.toml")),
    ("dayfox-light", include_str!("../../themes/dayfox-light.toml")),
    ("disciple-light", include_str!("../../themes/disciple-light.toml")),
    ("dracula-dark", include_str!("../../themes/dracula-dark.toml")),
    ("dracula-light", include_str!("../../themes/dracula-light.toml")),
    ("duskfox-dark", include_str!("../../themes/duskfox-dark.toml")),
    ("embark-dark", include_str!("../../themes/embark-dark.toml")),
    ("everforest-dark", include_str!("../../themes/everforest-dark.toml")),
    ("everforest-light", include_str!("../../themes/everforest-light.toml")),
    ("evergarden-dark", include_str!("../../themes/evergarden-dark.toml")),
    ("gruvbox-dark", include_str!("../../themes/gruvbox-dark.toml")),
    ("gruvbox-light", include_str!("../../themes/gruvbox-light.toml")),
    ("gruvbox-material-light", include_str!("../../themes/gruvbox-material-light.toml")),
    ("iambitchx-dark", include_str!("../../themes/iambitchx-dark.toml")),
    ("iceberg-dark", include_str!("../../themes/iceberg-dark.toml")),
    ("iceberg-light", include_str!("../../themes/iceberg-light.toml")),
    ("jellybeans-dark", include_str!("../../themes/jellybeans-dark.toml")),
    ("jellybeans-light", include_str!("../../themes/jellybeans-light.toml")),
    ("kanagawa-dark", include_str!("../../themes/kanagawa-dark.toml")),
    ("kanagawa-dragon-dark", include_str!("../../themes/kanagawa-dragon-dark.toml")),
    ("kanagawa-light", include_str!("../../themes/kanagawa-light.toml")),
    ("kanagawa-paper-dark", include_str!("../../themes/kanagawa-paper-dark.toml")),
    ("kanagawa-paper-light", include_str!("../../themes/kanagawa-paper-light.toml")),
    ("monokai-dark", include_str!("../../themes/monokai-dark.toml")),
    ("moonfly-dark", include_str!("../../themes/moonfly-dark.toml")),
    ("nightfly-dark", include_str!("../../themes/nightfly-dark.toml")),
    ("nightfox-dark", include_str!("../../themes/nightfox-dark.toml")),
    ("nordfox-dark", include_str!("../../themes/nordfox-dark.toml")),
    ("okabe-ito-dark", include_str!("../../themes/okabe-ito-dark.toml")),
    ("papercolor-dark", include_str!("../../themes/papercolor-dark.toml")),
    ("papercolor-light", include_str!("../../themes/papercolor-light.toml")),
    ("rose-pine-dark", include_str!("../../themes/rose-pine-dark.toml")),
    ("rose-pine-dawn-light", include_str!("../../themes/rose-pine-dawn-light.toml")),
    ("rose-pine-moon-dark", include_str!("../../themes/rose-pine-moon-dark.toml")),
    ("rosebones-dark", include_str!("../../themes/rosebones-dark.toml")),
    ("rosebones-light", include_str!("../../themes/rosebones-light.toml")),
    ("seoul256-dark", include_str!("../../themes/seoul256-dark.toml")),
    ("seoul256-light", include_str!("../../themes/seoul256-light.toml")),
    ("solarized-dark", include_str!("../../themes/solarized-dark.toml")),
    ("solarized-light", include_str!("../../themes/solarized-light.toml")),
    ("srcery-dark", include_str!("../../themes/srcery-dark.toml")),
    ("terafox-dark", include_str!("../../themes/terafox-dark.toml")),
    ("tokyobones-dark", include_str!("../../themes/tokyobones-dark.toml")),
    ("tokyobones-light", include_str!("../../themes/tokyobones-light.toml")),
    ("tokyonight-dark", include_str!("../../themes/tokyonight-dark.toml")),
    ("tokyonight-light", include_str!("../../themes/tokyonight-light.toml")),
    ("tokyonight-moon-dark", include_str!("../../themes/tokyonight-moon-dark.toml")),
    ("tokyonight-night-dark", include_str!("../../themes/tokyonight-night-dark.toml")),
    ("tokyonight-storm-dark", include_str!("../../themes/tokyonight-storm-dark.toml")),
    ("zenburn-dark", include_str!("../../themes/zenburn-dark.toml")),
];

/// A named palette.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Theme {
    pub name: String,
    pub palette: Palette,
}

/// Every theme `/theme` can pick, `bitchx` first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Themes {
    pub list: Vec<Theme>,
}

impl Default for Themes {
    fn default() -> Self {
        Themes {
            list: vec![
                Theme {
                    name: DEFAULT_THEME.into(),
                    palette: Palette::bitchx(),
                },
                Theme {
                    name: TERMINAL_THEME.into(),
                    palette: Palette::terminal(),
                },
            ],
        }
    }
}

/// The value at the dotted `path` of a TOML table.
fn get<'a>(t: &'a toml::Table, path: &str) -> Option<&'a toml::Value> {
    let mut parts = path.split('.');
    let mut v = t.get(parts.next()?)?;
    for p in parts {
        v = v.as_table()?.get(p)?;
    }
    Some(v)
}

/// Parse an iamb theme file into a palette (see the module docs).
pub(crate) fn parse_theme(src: &str) -> Result<Palette, String> {
    let doc: toml::Table = src.parse().map_err(|e: toml::de::Error| e.message().to_string())?;
    if !doc.get("theme").is_some_and(toml::Value::is_table) {
        return Err("no [theme] table".into());
    }
    let color = |path: &str| -> Result<Option<Color>, String> {
        match get(&doc, path) {
            None => Ok(None),
            Some(toml::Value::String(s)) => s
                .parse::<Color>()
                .map(Some)
                .map_err(|_| format!("{path}: bad colour {s:?}")),
            Some(_) => Err(format!("{path}: not a colour string")),
        }
    };
    let bg1 = color("theme.default.background")?;
    let bg2 = color("theme.messages.code.background")?;
    let ui1 = color("theme.windows.border.color")?;
    let ui2 = color("theme.windows.title.color")?;
    let warn1 = color("theme.cmdbar.error.color")?;
    let warn2 = color("theme.timeline.redacted.color")?;
    let warn3 = color("theme.encryption.icon_unknown.color")?;
    let good1 = color("theme.encryption.icon_encrypted.color")?;
    let text1 = color("theme.default.color")?;
    let text2 = color("theme.messages.code.color")?;
    let text3 = color("theme.messages.strong.color")?;
    let text4 = color("theme.messages.emphasis.color")?;
    let text5 = color("theme.rooms.default.color")?;

    let d = Palette::bitchx();
    let or = |c: Option<Color>, def: Color| c.unwrap_or(def);
    Ok(Palette {
        text: text1.or(d.text),
        background: bg1.or(d.background),
        punct: or(ui1, d.punct),
        bright: or(text5, d.bright),
        accent: or(text2, d.accent),
        nick: or(ui2, d.nick),
        own_nick: or(text3, d.own_nick),
        highlight: or(warn3, d.highlight),
        query_out: or(warn1, d.query_out),
        query_in: or(text3, d.query_in),
        join: or(text2, d.join),
        op: or(text2, d.op),
        good: or(good1, d.good),
        value: or(text4, d.value),
        error: or(warn1, d.error),
        debug: or(warn2, d.debug),
        debug_text: or(ui1, d.debug_text),
        timestamp: text4.or(d.timestamp),
        input: text1.or(d.input),
        logo: or(ui2, d.logo),
        logo_ink: [
            or(bg1, d.logo_ink[0]),
            or(warn1, d.logo_ink[1]),
            or(ui2, d.logo_ink[2]),
            or(text3, d.logo_ink[3]),
            or(text2, d.logo_ink[4]),
            or(text5, d.logo_ink[5]),
        ],
        status_bg: or(bg2, d.status_bg),
        status_fg: or(text4, d.status_fg),
        status_punct: or(ui2, d.status_punct),
        status_bright: or(text1, d.status_bright),
        status_away: or(good1, d.status_away),
        status_hilite: or(warn3, d.status_hilite),
        status_alert: or(warn1, d.status_alert),
    })
}

impl Themes {
    /// `bitchx`, `terminal`, the built-in themes, then the `*.toml` of `user_dir`
    /// (sorted; one with a built-in name replaces it). Invalid files are
    /// skipped; the returned lines say which and why.
    pub(crate) fn load(user_dir: Option<&Path>) -> (Self, Vec<String>) {
        let mut themes = Themes::default();
        let mut notes = vec![];
        for (name, src) in BUILTIN {
            match parse_theme(src) {
                Ok(palette) => themes.add(name, palette),
                Err(e) => notes.push(format!("Theme {name}: skipped ({e})")),
            }
        }
        let Some(dir) = user_dir else {
            return (themes, notes);
        };
        let mut files: Vec<_> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "toml"))
                .collect(),
            Err(_) => return (themes, notes),
        };
        files.sort();
        for path in files {
            let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
                continue;
            };
            let parsed = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|s| parse_theme(&s));
            match parsed {
                Ok(_) if name == DEFAULT_THEME || name == TERMINAL_THEME => {
                    notes.push(format!("Theme {}: skipped ({name} is built in)", path.display()));
                },
                Ok(palette) => themes.add(&name, palette),
                Err(e) => notes.push(format!("Theme {}: skipped ({e})", path.display())),
            }
        }
        (themes, notes)
    }

    fn add(&mut self, name: &str, palette: Palette) {
        match self.list.iter_mut().find(|t| t.name == name) {
            Some(t) => t.palette = palette,
            None => self.list.push(Theme { name: name.to_string(), palette }),
        }
    }

    /// The index of the theme called `name` (case-insensitive).
    pub(crate) fn position(&self, name: &str) -> Option<usize> {
        self.list.iter().position(|t| t.name.eq_ignore_ascii_case(name))
    }

    /// Every theme name, in list order.
    pub(crate) fn names(&self) -> Vec<String> {
        self.list.iter().map(|t| t.name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_list_matches_themes_dir() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str()?.strip_suffix(".toml").map(str::to_string))
            .collect();
        on_disk.sort();
        let builtin: Vec<String> = BUILTIN.iter().map(|(n, _)| (*n).to_string()).collect();
        assert_eq!(builtin, on_disk);
        assert_eq!(BUILTIN.len(), 62);
    }

    #[test]
    fn test_all_builtin_themes_parse() {
        let (themes, notes) = Themes::load(None);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(themes.list.len(), 64);
        assert_eq!(themes.list[0].name, DEFAULT_THEME);
        assert_eq!(themes.list[0].palette, Palette::bitchx());
        // bitchx, terminal, then the upstream themes alphabetically.
        assert_eq!(themes.list[1].name, TERMINAL_THEME);
        assert_eq!(themes.list[1].palette, Palette::terminal());
        let rest: Vec<&str> = themes.list[2..].iter().map(|t| t.name.as_str()).collect();
        let mut sorted = rest.clone();
        sorted.sort_unstable();
        assert_eq!(rest, sorted);
        assert_eq!(themes.position("GRUVBOX-dark"), themes.position("gruvbox-dark"));
    }

    #[test]
    fn test_theme_mapping() {
        let rgb = |s: &str| s.parse::<Color>().unwrap();
        let src = BUILTIN.iter().find(|(n, _)| *n == "apprentice-dark").unwrap().1;
        let p = parse_theme(src).unwrap();
        assert_eq!(p.background, Some(rgb("#1C1C1C")));
        assert_eq!(p.text, Some(rgb("#FFFFAF")));
        assert_eq!(p.punct, rgb("#585858"));
        assert_eq!(p.nick, rgb("#5F87AF"));
        assert_eq!(p.error, rgb("#AF5F5F"));
        assert_eq!(p.highlight, rgb("#87875F"));
        assert_eq!(p.good, rgb("#5F875F"));
        assert_eq!(p.accent, rgb("#5FAFAF"));
        assert_eq!(p.status_bg, rgb("#262626"));

        let src = BUILTIN.iter().find(|(n, _)| *n == "gruvbox-dark").unwrap().1;
        let g = parse_theme(src).unwrap();
        assert_ne!(g, Palette::bitchx());
        assert_ne!(g, p);
        assert!(matches!(g.status_bg, Color::Rgb(..)));
    }

    #[test]
    fn test_partial_theme_keeps_bitchx_roles() {
        let p = parse_theme("[theme.default]\ncolor = \"#102030\"\n").unwrap();
        assert_eq!(p.text, Some(Color::Rgb(0x10, 0x20, 0x30)));
        assert_eq!(p.status_bg, Palette::bitchx().status_bg);
        assert_eq!(p.background, None);
    }

    #[test]
    fn test_invalid_theme_files_are_skipped() {
        assert!(parse_theme("not toml [[[").is_err());
        assert!(parse_theme("[other]\na = 1\n").is_err());
        assert!(parse_theme("[theme.default]\ncolor = \"#zzzzzz\"\n").is_err());
        assert!(parse_theme("[theme.default]\ncolor = 7\n").is_err());

        let dir = std::env::temp_dir().join(format!("bx-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken.toml"), "[theme.default]\ncolor = \"nope\"\n").unwrap();
        std::fs::write(dir.join("garbage.toml"), [0xff, 0xfe, 0x00]).unwrap();
        std::fs::write(dir.join("terminal.toml"), "[theme.default]\ncolor = \"red\"\n").unwrap();
        std::fs::write(dir.join("mine.toml"), "[theme.default]\ncolor = \"#010203\"\n").unwrap();
        std::fs::write(dir.join("gruvbox-dark.toml"), "[theme.default]\ncolor = \"red\"\n")
            .unwrap();
        std::fs::write(dir.join("README.md"), "not a theme").unwrap();
        let (themes, notes) = Themes::load(Some(dir.as_path()));
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("terminal is built in")), "{notes:?}");
        assert!(notes.iter().all(|n| n.starts_with("Theme ") && n.contains("skipped")));
        assert_eq!(themes.list.len(), 65);
        assert_eq!(themes.list.last().unwrap().name, "mine");
        let g = &themes.list[themes.position("gruvbox-dark").unwrap()];
        assert_eq!(g.palette.text, Some(Color::Red));
        // A missing directory is no error.
        let (t, n) = Themes::load(Some(&dir.join("nope")));
        assert_eq!((t.list.len(), n.len()), (64, 0));
    }
}
