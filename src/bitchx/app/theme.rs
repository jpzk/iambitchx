//! `/theme`: switching palettes, and the live-preview picker docked above
//! the status lines.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};

use crate::bitchx::app::App;
use crate::bitchx::palette::set_palette;
use crate::bitchx::theme::Themes;

/// Rows the picker takes: a title row and [`PICKER_ITEMS`] names.
pub(crate) const PICKER_ROWS: usize = 5;
/// Theme names visible at once.
pub(crate) const PICKER_ITEMS: usize = PICKER_ROWS - 1;

/// The open `/theme` picker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Picker {
    /// The highlighted (and previewed) theme, an index into `App::themes`.
    pub sel: usize,
    /// The first visible entry.
    pub top: usize,
    /// The theme in use when the picker opened, restored by Esc.
    pub orig: String,
}

impl Picker {
    /// Scroll so `sel` is visible.
    fn follow(&mut self) {
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + PICKER_ITEMS {
            self.top = self.sel + 1 - PICKER_ITEMS;
        }
    }
}

impl App {
    /// Load the theme list (built-ins plus `<config dir>/themes`), print a
    /// line per skipped file and apply the configured `THEME`.
    pub(crate) fn load_themes(&mut self, config_dir: Option<&Path>) {
        let (themes, notes) = Themes::load(config_dir.map(|d| d.join("themes")).as_deref());
        self.themes = themes;
        for n in notes {
            self.info(n);
        }
        let want = self.vars.theme.clone();
        if !self.show_theme(&want) {
            self.info(format!("Unknown theme {want}, using bitchx (/theme lists them)"));
            self.show_theme(crate::bitchx::theme::DEFAULT_THEME);
        }
    }

    /// Draw with the theme called `name` from now on; `false` if unknown.
    fn show_theme(&mut self, name: &str) -> bool {
        let Some(i) = self.themes.position(name) else {
            return false;
        };
        set_palette(self.themes.list[i].palette);
        true
    }

    /// Switch to theme `name` and persist it as `THEME`.
    fn apply_theme(&mut self, name: &str) {
        let Some(i) = self.themes.position(name) else {
            return;
        };
        let name = self.themes.list[i].name.clone();
        self.show_theme(&name);
        self.vars.theme.clone_from(&name);
        self.info(format!("Theme set to {name}"));
        match self.config_toml.clone() {
            Some(path) => {
                if let Err(e) = self.vars.persist(&path) {
                    self.error(format!("Could not save {}: {e:#}", path.display()));
                }
            },
            None => self.error("Not saved: no config.toml (JSON config)"),
        }
    }

    /// `/theme [name]`.
    pub(crate) fn theme_cmd(&mut self, name: Option<String>) {
        match name {
            None => self.open_picker(),
            Some(n) if self.themes.position(&n).is_some() => self.apply_theme(&n),
            Some(n) => self.error(format!(
                "No such theme \"{n}\": /theme opens a list of them, Tab completes names"
            )),
        }
    }

    fn open_picker(&mut self) {
        let sel = self.themes.position(&self.vars.theme).unwrap_or(0);
        let mut p = Picker { sel, top: 0, orig: self.vars.theme.clone() };
        p.follow();
        self.picker = Some(p);
    }

    /// A key while the picker is open. Every key is consumed: nothing
    /// reaches the input line or the room.
    pub(super) fn picker_key(&mut self, key: KeyEvent) {
        let Some(mut p) = self.picker.take() else {
            return;
        };
        let last = self.themes.list.len().saturating_sub(1);
        let sel = match key.code {
            KeyCode::Up => p.sel.saturating_sub(1),
            KeyCode::Down => (p.sel + 1).min(last),
            KeyCode::PageUp => p.sel.saturating_sub(PICKER_ITEMS),
            KeyCode::PageDown => (p.sel + PICKER_ITEMS).min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            KeyCode::Enter => {
                let name = self.themes.list[p.sel].name.clone();
                self.apply_theme(&name);
                return;
            },
            KeyCode::Esc => {
                self.show_theme(&p.orig);
                return;
            },
            _ => p.sel,
        };
        if sel != p.sel {
            p.sel = sel;
            p.follow();
            set_palette(self.themes.list[sel].palette);
        }
        self.picker = Some(p);
    }

    /// Theme names for Tab after `/theme `.
    pub(crate) fn theme_names(&self) -> Vec<String> {
        self.themes.names()
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::bitchx::app::tests::{app, key, typed};
    use crate::bitchx::model::Line;
    use crate::bitchx::palette::{Palette, pal};

    fn palette_of(app: &crate::bitchx::app::App, name: &str) -> Palette {
        app.themes.list[app.themes.position(name).unwrap()].palette
    }

    fn temp_config(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bx-theme-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "default_profile = \"x\"\n[profiles.x]\nuser_id = \"@a:b\"\n")
            .unwrap();
        path
    }

    fn saved_theme(path: &std::path::Path) -> Option<String> {
        let doc: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
        doc.get("settings")?
            .get("bitchx")?
            .get("theme")?
            .as_str()
            .map(str::to_string)
    }

    #[test]
    fn test_picker_open_move_preview_enter_persists() {
        let mut app = app();
        let path = temp_config("enter");
        app.config_toml = Some(path.clone());
        assert!(typed(&mut app, "/theme").is_empty());
        let p = app.picker.clone().expect("picker open");
        assert_eq!((p.sel, p.orig.as_str()), (0, "bitchx"));
        assert_eq!(pal(), Palette::bitchx());

        // Down previews the next theme at once.
        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.picker.as_ref().unwrap().sel, 1);
        let first = app.themes.list[1].name.clone();
        assert_eq!(pal(), palette_of(&app, &first));
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.picker.as_ref().unwrap().sel, 1);
        // End / Home / PgDn / PgUp, clamped at both ends.
        app.handle_key(key(KeyCode::End));
        let last = app.themes.list.len() - 1;
        assert_eq!(app.picker.as_ref().unwrap().sel, last);
        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.picker.as_ref().unwrap().sel, last);
        assert!(app.picker.as_ref().unwrap().top + super::PICKER_ITEMS > last);
        app.handle_key(key(KeyCode::Home));
        assert_eq!(app.picker.as_ref().unwrap().sel, 0);
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.picker.as_ref().unwrap().sel, 0);
        app.handle_key(key(KeyCode::PageDown));
        assert_eq!(app.picker.as_ref().unwrap().sel, super::PICKER_ITEMS);
        app.handle_key(key(KeyCode::PageUp));
        app.handle_key(key(KeyCode::Down));

        // Typed keys go nowhere while the picker is open.
        let before = app.cur().lines.len();
        for c in "hello".chars() {
            assert!(app.handle_key(key(KeyCode::Char(c))).is_empty());
        }
        assert!(app.input.is_empty());
        assert_eq!(app.cur().lines.len(), before);

        // Enter keeps the previewed theme and saves it.
        assert!(app.handle_key(key(KeyCode::Enter)).is_empty());
        assert!(app.picker.is_none());
        assert!(app.input.is_empty(), "Enter must not submit the input line");
        assert_eq!(app.vars.theme, first);
        assert_eq!(pal(), palette_of(&app, &first));
        assert_eq!(saved_theme(&path).as_deref(), Some(first.as_str()));
        assert_eq!(app.cur().lines.last(), Some(&Line::Info(format!("Theme set to {first}"))));
        crate::bitchx::palette::set_palette(Palette::bitchx());
    }

    #[test]
    fn test_picker_esc_restores() {
        let mut app = app();
        let path = temp_config("esc");
        app.config_toml = Some(path.clone());
        typed(&mut app, "/theme gruvbox-dark");
        assert_eq!(app.vars.theme, "gruvbox-dark");
        let gruv = palette_of(&app, "gruvbox-dark");
        assert_eq!(pal(), gruv);

        typed(&mut app, "/theme");
        let p = app.picker.clone().unwrap();
        assert_eq!(p.orig, "gruvbox-dark");
        assert_eq!(app.themes.list[p.sel].name, "gruvbox-dark");
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Down));
        assert_ne!(pal(), gruv);
        app.handle_key(key(KeyCode::Esc));
        assert!(app.picker.is_none());
        assert_eq!(pal(), gruv);
        assert_eq!(app.vars.theme, "gruvbox-dark");
        assert_eq!(saved_theme(&path).as_deref(), Some("gruvbox-dark"));
        // Keys reach the input line again.
        app.handle_key(key(KeyCode::Char('x')));
        assert_eq!(app.input.text(), "x");
        crate::bitchx::palette::set_palette(Palette::bitchx());
    }

    #[test]
    fn test_theme_by_name_unknown_and_completion() {
        let mut app = app();
        typed(&mut app, "/theme no-such-theme");
        let Some(Line::Error(e)) = app.cur().lines.last().cloned() else {
            panic!("no error line");
        };
        assert!(e.contains("no-such-theme") && e.contains("/theme"), "{e}");
        assert_eq!(pal(), Palette::bitchx());
        assert!(app.picker.is_none());

        for c in "/theme gruvbox-d".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.input.text(), "/theme gruvbox-dark ");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.vars.theme, "gruvbox-dark");
        assert!(app.cur().lines.contains(&Line::Info("Theme set to gruvbox-dark".into())));
        assert_eq!(
            app.cur().lines.last(),
            Some(&Line::Error("Not saved: no config.toml (JSON config)".into()))
        );
        crate::bitchx::palette::set_palette(Palette::bitchx());
    }

    #[test]
    fn test_startup_theme_from_config() {
        let mut app = app();
        app.vars.theme = "dracula-dark".into();
        app.load_themes(None);
        assert_eq!(pal(), palette_of(&app, "dracula-dark"));
        // An unknown configured theme falls back to bitchx with a note.
        app.vars.theme = "gone".into();
        app.load_themes(None);
        assert_eq!(pal(), Palette::bitchx());
        assert!(matches!(app.cur().lines.last(), Some(Line::Info(t)) if t.contains("gone")));
    }

    #[test]
    fn test_picker_drawn_above_status_lines() {
        let mut app = app();
        typed(&mut app, "/theme");
        app.handle_key(key(KeyCode::Down));
        let mut term = Terminal::new(TestBackend::new(60, 16)).unwrap();
        term.draw(|f| crate::bitchx::ui::draw(f, &mut app)).unwrap();
        let buf = term.backend().buffer().clone();
        let row = |y: u16| (0..60).map(|x| buf[(x, y)].symbol()).collect::<String>();
        // 16 rows: scrollback 0..=7, picker 8..=12, status 13..=14, input 15.
        assert!(row(8).contains("Theme 2/64"), "{}", row(8));
        assert!(row(9).contains(" * bitchx"), "{}", row(9));
        let second = app.themes.list[1].name.clone();
        assert!(row(10).contains(&format!(">  {second}")), "{}", row(10));
        assert!(row(15).starts_with("[status]"), "{}", row(15));
        crate::bitchx::palette::set_palette(Palette::bitchx());
    }
}
