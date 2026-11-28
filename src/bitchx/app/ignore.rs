//! `/ignore` and `/unignore`: the Matrix account ignore list
//! (`m.ignored_user_list`), plus client-side hiding of ignored senders in
//! case the server still delivers their events.
use matrix_sdk::ruma::OwnedUserId;

use crate::bitchx::app::App;
use crate::bitchx::model::{Effect, Line, nick_of};

impl App {
    /// `/ignore` lists, `/ignore NICK|@user:server` ignores.
    pub(super) fn ignore(&mut self, target: Option<String>) -> Vec<Effect> {
        let Some(target) = target else {
            self.ignore_list();
            return vec![];
        };
        let Some(user) = self.user_cmd(&target) else {
            return vec![];
        };
        if user == self.me {
            self.error("You can't ignore yourself");
            return vec![];
        }
        if self.ignored.contains(&user) {
            self.info(format!("{user} is already being ignored"));
            return vec![];
        }
        vec![Effect::Ignore(user)]
    }

    /// `/unignore NICK|@user:server`: the nick may also name an ignored
    /// user who is in no window any more.
    pub(super) fn unignore(&mut self, target: &str) -> Vec<Effect> {
        let user = match self.ignored_by_nick(target) {
            Some(u) => u,
            None => match self.user_cmd(target) {
                Some(u) => u,
                None => return vec![],
            },
        };
        if !self.ignored.contains(&user) {
            self.info(format!("{user} is not being ignored"));
            return vec![];
        }
        vec![Effect::Unignore(user)]
    }

    fn ignored_by_nick(&self, target: &str) -> Option<OwnedUserId> {
        if target.starts_with('@') {
            return None;
        }
        let t = target.to_lowercase();
        let hits: Vec<&OwnedUserId> = self
            .ignored
            .iter()
            .filter(|u| nick_of(u).to_lowercase() == t || u.localpart().to_lowercase() == t)
            .collect();
        match hits.as_slice() {
            [one] => Some((*one).clone()),
            _ => None,
        }
    }

    fn ignore_list(&mut self) {
        if self.ignored.is_empty() {
            self.info("There are no nicknames being ignored");
            return;
        }
        self.info("Ignorance List:");
        let rows: Vec<String> = self
            .ignored
            .iter()
            .enumerate()
            .map(|(i, u)| format!("{:>3}  {:<16} {u}", i + 1, nick_of(u)))
            .collect();
        for r in rows {
            self.info(r);
        }
    }

    /// The account ignore list changed (sync, or our own `/ignore`): print
    /// who was added or removed, except for the first list at startup.
    pub(super) fn ignored_changed(&mut self, users: Vec<OwnedUserId>) {
        let added: Vec<OwnedUserId> =
            users.iter().filter(|u| !self.ignored.contains(u)).cloned().collect();
        let removed: Vec<OwnedUserId> =
            self.ignored.iter().filter(|u| !users.contains(u)).cloned().collect();
        let mut users = users;
        users.sort();
        users.dedup();
        self.ignored = users;
        if !self.ignore_known {
            // the first list at startup: silent
            self.ignore_known = true;
            return;
        }
        for u in added {
            self.info(format!("Now ignoring ALL from {} ({u})", nick_of(&u)));
        }
        for u in removed {
            self.info(format!("{} ({u}) removed from ignorance list", nick_of(&u)));
        }
    }

    /// The line was sent by an ignored user and must not be shown.
    pub(super) fn from_ignored(&self, line: &Line) -> bool {
        if self.ignored.is_empty() {
            return false;
        }
        match line {
            Line::Private { own: false, mxid, .. } | Line::Notice { mxid, .. } => {
                self.ignored.iter().any(|u| u.as_str() == mxid)
            },
            Line::Public { own: false, nick, .. }
            | Line::Action { own: false, nick, .. }
            | Line::Undecryptable { nick, .. } => self.ignored.iter().any(|u| nick_of(u) == *nick),
            _ => false,
        }
    }
}
