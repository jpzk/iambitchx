//! # IRC channel modes on Matrix
//!
//! `/mode` maps the IRC channel modes onto Matrix room state:
//!
//! - `+i`/`-i`: `m.room.join_rules` `invite` / `public`
//! - `+m`/`-m`: `events_default` power [`MODERATED`] / 0 (moderated)
//! - `+t`/`-t`: power needed for `m.room.topic` [`TOPIC_LOCK`] / 0
//! - `+o`/`-o nick`: the user's power level [`OP`] / 0 (as `/op`, `/deop`)
//! - `+b`/`-b nick`: ban / unban
//!
//! Pure: nicks are resolved and state is sent elsewhere.

/// The power level `/op` and `+o` give.
pub(crate) const OP: i64 = 50;
/// `events_default` while a room is moderated (`+m`).
pub(crate) const MODERATED: i64 = 50;
/// The power `m.room.topic` needs while the topic is locked (`+t`).
pub(crate) const TOPIC_LOCK: i64 = 50;

/// The mode letters `/mode` can set, for errors and help.
pub(crate) const SUPPORTED: &str = "+i +m +t +o +b";

/// The modes a room has, as far as IRC can tell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RoomModes {
    pub invite_only: bool,
    pub moderated: bool,
    pub topic_lock: bool,
}

impl RoomModes {
    /// From the power levels: moderated when a default user cannot talk,
    /// topic-locked when a default user cannot set the topic.
    pub(crate) fn from_levels(
        invite_only: bool,
        users_default: i64,
        events_default: i64,
        topic: i64,
    ) -> Self {
        RoomModes {
            invite_only,
            moderated: events_default > users_default,
            topic_lock: topic > users_default,
        }
    }

    /// The letters, in IRC order: `imt`.
    pub(crate) fn letters(&self) -> String {
        let mut s = String::new();
        for (on, c) in [
            (self.invite_only, 'i'),
            (self.moderated, 'm'),
            (self.topic_lock, 't'),
        ] {
            if on {
                s.push(c);
            }
        }
        s
    }

    /// `+imt` (bare `/mode`); `+` when none is set.
    pub(crate) fn irc(&self) -> String {
        format!("+{}", self.letters())
    }
}

/// One mode change from a `/mode` line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ModeChange {
    InviteOnly(bool),
    Moderated(bool),
    TopicLock(bool),
    Op { on: bool, nick: String },
    Ban { on: bool, nick: String },
}

/// What a [`ModeChange`] does to Matrix state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MatrixChange {
    /// `m.room.join_rules`: `invite` (`true`) or `public`.
    JoinRule {
        invite: bool,
    },
    /// `m.room.power_levels` `events_default`.
    EventsDefault(i64),
    /// `m.room.power_levels` `events["m.room.topic"]`.
    TopicLevel(i64),
    /// `m.room.power_levels` `users[nick]`.
    UserPower {
        nick: String,
        level: i64,
    },
    Ban {
        nick: String,
    },
    Unban {
        nick: String,
    },
}

impl ModeChange {
    /// `+i`, `-o bob`, ... as IRC shows it.
    pub(crate) fn irc(&self) -> String {
        let sign = |on: bool| if on { '+' } else { '-' };
        match self {
            ModeChange::InviteOnly(on) => format!("{}i", sign(*on)),
            ModeChange::Moderated(on) => format!("{}m", sign(*on)),
            ModeChange::TopicLock(on) => format!("{}t", sign(*on)),
            ModeChange::Op { on, nick } => format!("{}o {nick}", sign(*on)),
            ModeChange::Ban { on, nick } => format!("{}b {nick}", sign(*on)),
        }
    }

    /// The Matrix state change this mode stands for.
    pub(crate) fn to_matrix(&self) -> MatrixChange {
        let level = |on: bool, l: i64| if on { l } else { 0 };
        match self {
            ModeChange::InviteOnly(on) => MatrixChange::JoinRule { invite: *on },
            ModeChange::Moderated(on) => MatrixChange::EventsDefault(level(*on, MODERATED)),
            ModeChange::TopicLock(on) => MatrixChange::TopicLevel(level(*on, TOPIC_LOCK)),
            ModeChange::Op { on, nick } => {
                MatrixChange::UserPower { nick: nick.clone(), level: level(*on, OP) }
            },
            ModeChange::Ban { on: true, nick } => MatrixChange::Ban { nick: nick.clone() },
            ModeChange::Ban { on: false, nick } => MatrixChange::Unban { nick: nick.clone() },
        }
    }
}

/// Parse `+im-t+o bob`: mode words start with `+` or `-`; every other word
/// is an argument, handed to the `o`/`b` letters in order (IRC style:
/// `+ob-o alice bob carol`).
pub(crate) fn parse_modes(s: &str) -> Result<Vec<ModeChange>, String> {
    let mut letters = vec![];
    let mut args = std::collections::VecDeque::new();
    for word in s.split_whitespace() {
        if word.starts_with('+') || word.starts_with('-') {
            let mut on = true;
            for c in word.chars() {
                match c {
                    '+' => on = true,
                    '-' => on = false,
                    c => letters.push((on, c)),
                }
            }
        } else {
            args.push_back(word.to_string());
        }
    }
    if letters.is_empty() {
        return Err(format!("No mode given (supported: {SUPPORTED})"));
    }
    let mut out = vec![];
    for (on, c) in letters {
        let mut nick = || {
            args.pop_front()
                .ok_or_else(|| format!("{}{c} needs a nick", if on { '+' } else { '-' }))
        };
        out.push(match c {
            'i' => ModeChange::InviteOnly(on),
            'm' => ModeChange::Moderated(on),
            't' => ModeChange::TopicLock(on),
            'o' => ModeChange::Op { on, nick: nick()? },
            'b' => ModeChange::Ban { on, nick: nick()? },
            'e' => {
                return Err(format!(
                    "Unknown mode 'e' (supported: {SUPPORTED}); end-to-end encryption is /encrypt"
                ));
            },
            c => return Err(format!("Unknown mode '{c}' (supported: {SUPPORTED})")),
        });
    }
    if let Some(extra) = args.front() {
        return Err(format!("Too many arguments: {extra}"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> Vec<MatrixChange> {
        parse_modes(s)
            .unwrap_or_else(|e| panic!("{s}: {e}"))
            .iter()
            .map(ModeChange::to_matrix)
            .collect()
    }

    fn nick(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn test_mode_i_join_rule() {
        assert_eq!(m("+i"), vec![MatrixChange::JoinRule { invite: true }]);
        assert_eq!(m("-i"), vec![MatrixChange::JoinRule { invite: false }]);
    }

    #[test]
    fn test_mode_m_events_default() {
        assert_eq!(m("+m"), vec![MatrixChange::EventsDefault(50)]);
        assert_eq!(m("-m"), vec![MatrixChange::EventsDefault(0)]);
    }

    #[test]
    fn test_mode_t_topic_level() {
        assert_eq!(m("+t"), vec![MatrixChange::TopicLevel(50)]);
        assert_eq!(m("-t"), vec![MatrixChange::TopicLevel(0)]);
    }

    #[test]
    fn test_mode_o_power() {
        assert_eq!(m("+o bob"), vec![MatrixChange::UserPower { nick: nick("bob"), level: 50 }]);
        assert_eq!(m("-o bob"), vec![MatrixChange::UserPower { nick: nick("bob"), level: 0 }]);
    }

    #[test]
    fn test_mode_b_ban() {
        assert_eq!(m("+b bob"), vec![MatrixChange::Ban { nick: nick("bob") }]);
        assert_eq!(m("-b bob"), vec![MatrixChange::Unban { nick: nick("bob") }]);
    }

    #[test]
    fn test_mode_combined_irc_style() {
        assert_eq!(
            m("+im-t"),
            vec![
                MatrixChange::JoinRule { invite: true },
                MatrixChange::EventsDefault(50),
                MatrixChange::TopicLevel(0),
            ]
        );
        assert_eq!(
            m("+ob-o alice bob carol"),
            vec![
                MatrixChange::UserPower { nick: nick("alice"), level: 50 },
                MatrixChange::Ban { nick: nick("bob") },
                MatrixChange::UserPower { nick: nick("carol"), level: 0 },
            ]
        );
        assert_eq!(m("+i +o bob"), m("+io bob"));
        let irc: Vec<_> = parse_modes("-b+o x y").unwrap().iter().map(ModeChange::irc).collect();
        assert_eq!(irc, vec!["-b x", "+o y"]);
    }

    #[test]
    fn test_mode_refusals() {
        let e = parse_modes("+n").unwrap_err();
        assert!(e.contains("'n'") && e.contains("+i +m +t +o +b"), "{e}");
        let e = parse_modes("+ix").unwrap_err();
        assert!(e.contains("'x'"), "{e}");
        assert!(parse_modes("+e").unwrap_err().contains("/encrypt"));
        assert_eq!(parse_modes("+o").unwrap_err(), "+o needs a nick");
        assert_eq!(parse_modes("-b").unwrap_err(), "-b needs a nick");
        assert_eq!(parse_modes("+i bob").unwrap_err(), "Too many arguments: bob");
        assert!(parse_modes("bob").unwrap_err().starts_with("No mode given"));
        assert!(parse_modes("+").unwrap_err().starts_with("No mode given"));
    }

    #[test]
    fn test_room_modes_string() {
        assert_eq!(RoomModes::default().irc(), "+");
        let all = RoomModes {
            invite_only: true,
            moderated: true,
            topic_lock: true,
        };
        assert_eq!(all.irc(), "+imt");
        assert_eq!(RoomModes { topic_lock: true, ..Default::default() }.letters(), "t");
        // Matrix defaults: users 0, events 0, state (topic) 50.
        assert_eq!(RoomModes::from_levels(true, 0, 0, 50).irc(), "+it");
        assert_eq!(RoomModes::from_levels(false, 0, 50, 0).irc(), "+m");
        // Everyone at 50 by default: nothing is locked.
        assert_eq!(RoomModes::from_levels(false, 50, 50, 50).irc(), "+");
    }
}
