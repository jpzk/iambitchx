//! Away handling: the AUTO_AWAY idle timer, AWAY_NICK display-name
//! swapping, the connect-time self-whois probe and persisting vars.

use crate::bitchx::app::{App, Probe};
use crate::bitchx::model::{Effect, Line};
use crate::bitchx::vars::{expand_auto_away_msg, expand_away_nick, strip_away_nick};

impl App {
    /// The timer: `check_auto_away` (`source/misc.c:1204-1225`). After
    /// AUTO_AWAY_TIME seconds without a keypress, go away with
    /// `Auto-Away after N mins`, unless already away (by hand or not).
    pub(crate) fn tick(&mut self) -> Vec<Effect> {
        self.flush_debug();
        let secs = self.vars.auto_away_time;
        let now = self.now();
        if !self.auto_away_armed
            || !self.vars.auto_away
            || secs == 0
            || self.away.is_some()
            || now - self.last_input < i64::from(secs) * 1000
        {
            return vec![];
        }
        let msg = expand_auto_away_msg(&self.vars.auto_away_msg, secs / 60);
        let nick = self.away_nick_on();
        self.auto_away = true;
        self.away = Some(msg.clone());
        self.away_since = Some(now);
        let mut fx = vec![Effect::Away(Some(msg))];
        fx.extend(nick);
        fx
    }

    /// Keyboard activity: resets the idle clock; ends an automatic away
    /// (a `/away` by hand stays until `/back`).
    pub(crate) fn activity(&mut self) -> Vec<Effect> {
        self.last_input = self.now();
        if self.auto_away && self.away.is_some() {
            self.auto_away = false;
            let mut fx = vec![Effect::Away(None)];
            fx.extend(self.away_nick_off());
            return fx;
        }
        vec![]
    }

    /// Going away: switch the display name to AWAY_NICK, remembering (and
    /// persisting) the original. Nothing while already away, when off, or
    /// while our display name is unknown. Never stacks: an already-away
    /// name (`carla^afk`) is unwrapped to its original first.
    pub(super) fn away_nick_on(&mut self) -> Vec<Effect> {
        let pattern = self.vars.away_nick.clone();
        if self.away.is_some() || self.vars.away_nick_orig.is_some() || pattern.is_empty() {
            return vec![];
        }
        let Some(cur) = self.display_name.clone() else {
            self.error("AWAY_NICK skipped: display name not known yet");
            return vec![];
        };
        let orig = strip_away_nick(&pattern, &cur).unwrap_or_else(|| cur.clone());
        let Some(nick) = expand_away_nick(&pattern, &orig) else {
            return vec![];
        };
        self.vars.away_nick_orig = Some(orig);
        self.save_vars();
        if nick == cur {
            return vec![];
        }
        vec![Effect::Nick(nick)]
    }

    /// Back: restore exactly the display name from before the away nick.
    pub(super) fn away_nick_off(&mut self) -> Vec<Effect> {
        let Some(orig) = self.vars.away_nick_orig.take() else {
            return vec![];
        };
        self.save_vars();
        if self.display_name.as_deref() == Some(orig.as_str()) {
            return vec![];
        }
        vec![Effect::Nick(orig)]
    }

    /// Our display name is now `name`: one ` iamBitchX:` line per change.
    pub(super) fn own_name(&mut self, name: String) {
        if self.display_name.as_deref() == Some(name.as_str()) {
            return;
        }
        let msg = match &self.vars.away_nick_orig {
            Some(orig) if *orig != name => {
                format!("Display name is now {name} (away; /back restores {orig})")
            },
            _ => format!("Display name is now {name}"),
        };
        self.display_name = Some(name);
        self.windows[0].push(None, Line::Error(msg));
    }

    /// Swallow the lines of the connect-time self-whois; `true` if `line`
    /// was one of them.
    pub(super) fn probe_line(&mut self, line: &Line) -> bool {
        match (self.probe, line) {
            (Probe::Pending { .. }, Line::WhoisRule) => {
                self.probe = Probe::Pending { rule: true };
                true
            },
            (Probe::Pending { .. }, Line::WhoisNick { mxid, .. }) if *mxid == self.me.as_str() => {
                self.probe = Probe::Block;
                true
            },
            (Probe::Pending { rule: true }, Line::WhoisNick { .. }) => {
                // someone else's whois: give back the rule we took
                self.probe = Probe::Pending { rule: false };
                self.cur_mut().push(None, Line::WhoisRule);
                false
            },
            (Probe::Block, Line::WhoisRow { key, value, last, .. }) => {
                if key == "ircname" {
                    self.display_name = Some(value.clone());
                }
                if *last {
                    self.probe = Probe::Done;
                    self.startup_offer();
                }
                true
            },
            _ => false,
        }
    }

    /// At connect, not away, but the display name is still the away nick
    /// (crash or restart while away): offer `/back` to restore it.
    pub(super) fn startup_offer(&mut self) {
        let (Some(cur), None) = (self.display_name.clone(), &self.away) else {
            return;
        };
        let pattern = self.vars.away_nick.clone();
        if self.vars.away_nick_orig.as_deref() == Some(cur.as_str()) {
            // already restored
            self.vars.away_nick_orig = None;
            self.save_vars();
            return;
        }
        if self.vars.away_nick_orig.is_none() {
            let Some(base) = strip_away_nick(&pattern, &cur) else {
                return;
            };
            self.vars.away_nick_orig = Some(base);
            self.save_vars();
        }
        let orig = self.vars.away_nick_orig.clone().unwrap_or_default();
        self.error(format!(
            "Display name is still the away nick {cur}: type /back to restore {orig}"
        ));
    }

    /// Persist [`Self::vars`] (the away nick original) if there is a
    /// `config.toml`; quietly skipped otherwise.
    pub(super) fn save_vars(&mut self) {
        if let Some(path) = self.config_toml.clone()
            && let Err(e) = self.vars.persist(&path)
        {
            self.error(format!("Could not save {}: {e:#}", path.display()));
        }
    }

    /// The homeserver confirmed an away change: the 306/305 lines, and on
    /// return BitchX's `You were /away for ...` (`source/commands.c:3124`).
    pub(super) fn away_changed(&mut self, away: Option<String>) {
        let now = self.now();
        match &away {
            Some(r) => {
                self.away_since.get_or_insert(now);
                self.info(format!("You have been marked as being away ({r})"));
            },
            None => {
                if let Some(since) = self.away_since.take() {
                    let t = (now - since).max(0) / 1000;
                    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
                    self.info(format!(
                        "You were /away for {h} hours {m} minutes and {s} seconds. [BX-MsgLog Off]"
                    ));
                }
                self.info("You are no longer marked as being away");
                self.auto_away = false;
                self.away_msgs = 0;
            },
        }
        self.away = away;
    }
}
