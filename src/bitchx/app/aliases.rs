//! `/alias` and `/bind`: user aliases and key binds, run through the same
//! path as a typed line, with expansion depth capped at [`MAX_DEPTH`].

use crate::bitchx::aliases::{Aliases, BindKey, Binds, MAX_DEPTH, persist_table};
use crate::bitchx::app::App;
use crate::bitchx::commands::parse;
use crate::bitchx::model::Effect;
use crate::bitchx::vars::BitchxConfig;

impl App {
    /// Load `[settings.bitchx.aliases]` / `[settings.bitchx.binds]`; bad
    /// entries are skipped with a message in the status window.
    pub(crate) fn load_aliases_binds(&mut self, cfg: &BitchxConfig) {
        let (aliases, mut warn) = Aliases::from_config(cfg.aliases.as_ref());
        let (binds, w) = Binds::from_config(cfg.binds.as_ref());
        warn.extend(w);
        self.aliases = aliases;
        self.binds = binds;
        for w in warn {
            self.error(w);
        }
    }

    /// Run a line as if typed: `/command` (aliases expanded), `//text`
    /// literally, anything else as text for the current window.
    pub(super) fn run_line(&mut self, line: &str, depth: usize) -> Vec<Effect> {
        if let Some(rest) = line.strip_prefix('/')
            && !rest.starts_with('/')
        {
            return self.run_command(rest, depth);
        }
        // `//text` sends `/text` literally.
        let text = line.strip_prefix('/').filter(|r| r.starts_with('/')).unwrap_or(line);
        self.say(text.to_string(), false)
    }

    /// Run the text after a leading `/`, expanding user aliases.
    fn run_command(&mut self, rest: &str, depth: usize) -> Vec<Effect> {
        if let Some(expanded) = self.aliases.expand_line(rest) {
            if depth >= MAX_DEPTH {
                self.error(format!("Alias recursion too deep (limit {MAX_DEPTH})"));
                return vec![];
            }
            return self.run_command(&expanded, depth + 1);
        }
        match parse(rest) {
            Ok(Some(cmd)) => self.command(cmd),
            Ok(None) => vec![],
            Err(e) => {
                self.error(e.to_string());
                vec![]
            },
        }
    }

    /// A bound key was pressed: the command it runs, if any.
    pub(super) fn bound_key(&mut self, key: &crossterm::event::KeyEvent) -> Option<Vec<Effect>> {
        let cmd = self.binds.get(BindKey::from_event(key)?)?.to_string();
        Some(self.run_line(&cmd, 0))
    }

    /// `/alias`: list; `/alias NAME`: show; `/alias -NAME`: remove;
    /// `/alias NAME body`: define (built-in commands cannot be shadowed).
    pub(super) fn user_alias(&mut self, name: Option<String>, body: Option<String>) {
        let Some(name) = name else {
            if self.aliases.is_empty() {
                return self.info("No aliases");
            }
            let lines: Vec<String> = self
                .aliases
                .iter()
                .map(|(n, b)| format!("{:<12} /{b}", n.to_uppercase()))
                .collect();
            self.info("Aliases:");
            for l in lines {
                self.info(l);
            }
            return;
        };
        if let Some(del) = name.strip_prefix('-') {
            match self.aliases.remove(del) {
                Some(_) => self.info(format!("Alias {} removed", del.to_uppercase())),
                None => return self.error(format!("No such alias {}", del.to_uppercase())),
            }
            return self.save_aliases();
        }
        let Some(body) = body else {
            let line = match self.aliases.get(&name) {
                Some(b) => format!("{:<12} /{b}", name.to_uppercase()),
                None => format!("No such alias {}", name.to_uppercase()),
            };
            return self.info(line);
        };
        match self.aliases.set(&name, &body) {
            Ok(n) => {
                let b = self.aliases.get(&n).unwrap_or_default().to_string();
                self.info(format!("Alias {} added: /{b}", n.to_uppercase()));
                self.save_aliases();
            },
            Err(e) => self.error(e),
        }
    }

    /// `/bind`: list; `/bind KEY`: show; `/bind -KEY`: unbind;
    /// `/bind KEY /command`: bind.
    pub(super) fn bind(&mut self, key: Option<String>, command: Option<String>) {
        let Some(key) = key else {
            if self.binds.is_empty() {
                return self.info("No key binds");
            }
            let lines: Vec<String> = self
                .binds
                .iter()
                .map(|(k, c)| format!("{:<6} {c}", k.to_string()))
                .collect();
            self.info("Key binds:");
            for l in lines {
                self.info(l);
            }
            return;
        };
        if let Some(del) = key.strip_prefix('-').filter(|d| !d.is_empty()) {
            match self.binds.remove(del) {
                Ok(Some((k, _))) => self.info(format!("{k} unbound")),
                Ok(None) => return self.error(format!("{del} is not bound")),
                Err(e) => return self.error(e),
            }
            return self.save_binds();
        }
        let Some(command) = command else {
            let line = match BindKey::parse(&key) {
                Ok(k) => match self.binds.get(k) {
                    Some(c) => format!("{k:<6} {c}"),
                    None => format!("{k} is not bound"),
                },
                Err(e) => return self.error(e),
            };
            return self.info(line);
        };
        match self.binds.set(&key, &command) {
            Ok(k) => {
                let c = self.binds.get(k).unwrap_or_default().to_string();
                self.info(format!("{k} bound to {c}"));
                self.save_binds();
            },
            Err(e) => self.error(e),
        }
    }

    fn save_aliases(&mut self) {
        let map = self.aliases.map().clone();
        self.save_table("aliases", &map);
    }

    fn save_binds(&mut self) {
        let map = self.binds.to_config();
        self.save_table("binds", &map);
    }

    fn save_table(&mut self, table: &str, map: &std::collections::BTreeMap<String, String>) {
        match self.config_toml.clone() {
            Some(path) => {
                if let Err(e) = persist_table(&path, table, map) {
                    self.error(format!("Could not save {}: {e:#}", path.display()));
                }
            },
            None => self.error("Not saved: no config.toml (JSON config)"),
        }
    }
}
