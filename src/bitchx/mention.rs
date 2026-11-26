//! # Mentions (BitchX `AUTO_REPLY`)
//!
//! BitchX1.3 `source/parse.c:596` `check_auto_reply` marks a public line
//! whose text contains one of the `AUTO_REPLY` words (default: our nick);
//! it is then shown with `FORMAT_PUBLIC_AR` (`%b<%Y$1%b>%n $3-`,
//! `include/color.h:199`) and beeps.
//!
//! Here a line is a mention when its body contains, case-insensitively and
//! on word boundaries, our mxid localpart (`carla`, also as `@carla`), our
//! full mxid (`@carla:server`), our current display name or one of the
//! extra `AUTO_REPLY` words. Our own lines never are. Matching is on the
//! body text only: `m.mentions` is not carried into the UI [`Line`]s.
use matrix_sdk::ruma::UserId;

use crate::bitchx::model::Line;

/// The words that highlight a line, lowercased.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Matcher {
    words: Vec<String>,
}

/// A character that is part of a word (so cannot be a boundary).
fn wordy(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The `AUTO_REPLY` value split into words: comma separated, trimmed,
/// empty entries dropped.
pub fn split_words(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

impl Matcher {
    pub fn new(me: &UserId, display_name: Option<&str>, auto_reply: &str) -> Self {
        let mut words: Vec<String> = vec![me.localpart().to_string(), me.to_string()];
        words.extend(display_name.map(str::trim).filter(|d| !d.is_empty()).map(str::to_string));
        words.extend(split_words(auto_reply));
        let mut words: Vec<String> = words.into_iter().map(|w| w.to_lowercase()).collect();
        words.sort();
        words.dedup();
        Matcher { words }
    }

    /// `text` contains one of the words on word boundaries. A leading `@`
    /// is a boundary, so `@carla` matches `carla`.
    pub fn matches(&self, text: &str) -> bool {
        let hay = text.to_lowercase();
        self.words.iter().any(|w| contains_word(&hay, w))
    }

    /// Whether `line` mentions us: someone else's message, notice or
    /// action whose text matches.
    pub fn is_mention(&self, line: &Line) -> bool {
        match line {
            Line::Public { own: false, text, .. } |
            Line::Action { own: false, text, .. } |
            Line::Private { own: false, text, .. } |
            Line::Notice { text, .. } => self.matches(text),
            _ => false,
        }
    }
}

/// `word` occurs in `hay` not glued to other word characters (checked only
/// on the sides where `word` itself starts/ends with a word character, so
/// `@carla:x.y` needs no boundary before the `@`).
fn contains_word(hay: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let first_wordy = word.chars().next().is_some_and(wordy);
    let last_wordy = word.chars().next_back().is_some_and(wordy);
    hay.match_indices(word).any(|(i, m)| {
        let before = hay[..i].chars().next_back();
        let after = hay[i + m.len()..].chars().next();
        let glued_before = first_wordy && before.is_some_and(wordy);
        let glued_after = last_wordy && after.is_some_and(wordy);
        !(glued_before || glued_after)
    })
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::owned_user_id;

    use super::*;

    fn m() -> Matcher {
        Matcher::new(&owned_user_id!("@carla:home-srv.test"), Some("Big Creed"), "")
    }

    fn public(text: &str, own: bool) -> Line {
        Line::Public { nick: "bob".into(), own, text: text.into() }
    }

    #[test]
    fn test_localpart_word_boundary() {
        let m = m();
        assert!(m.matches("carla"));
        assert!(m.matches("hey carla, sup"));
        assert!(m.matches("CARLA: look"));
        assert!(m.matches("ping @carla"));
        assert!(m.matches("(carla)"));
        assert!(!m.matches("carlax"));
        assert!(!m.matches("xcarla"));
        assert!(!m.matches("carla_bot"));
        assert!(!m.matches("cr3 3d"));
        assert!(!m.matches(""));
    }

    #[test]
    fn test_mxid_and_display_name() {
        let m = m();
        assert!(m.matches("@carla:home-srv.test"));
        assert!(m.matches("ask @CARLA:home-srv.test please"));
        assert!(m.matches("big creed did it"));
        assert!(m.matches("BIG CREED!"));
        assert!(!m.matches("big creedo"), "display name needs a boundary too");
        assert!(!m.matches("creed"), "half a display name is not it");
    }

    #[test]
    fn test_auto_reply_words() {
        let m = Matcher::new(&owned_user_id!("@carla:x.y"), None, " creed , ,bx ");
        assert!(m.matches("yo creed"));
        assert!(m.matches("BX rocks"));
        assert!(!m.matches("bxx"));
        assert_eq!(split_words(" a, ,b ,"), vec!["a", "b"]);
        assert_eq!(split_words(""), Vec::<String>::new());
    }

    #[test]
    fn test_own_lines_never_mention() {
        let m = m();
        assert!(m.is_mention(&public("hi carla", false)));
        assert!(!m.is_mention(&public("hi carla", true)));
        assert!(!m.is_mention(&public("hi all", false)));
        let act = |own| {
            Line::Action {
                nick: "bob".into(),
                own,
                text: "pokes carla".into(),
            }
        };
        assert!(m.is_mention(&act(false)));
        assert!(!m.is_mention(&act(true)));
        let q = |own| {
            Line::Private {
                nick: "bob".into(),
                mxid: "@bob:x".into(),
                own,
                text: "carla?".into(),
            }
        };
        assert!(m.is_mention(&q(false)));
        assert!(!m.is_mention(&q(true)));
        assert!(!m.is_mention(&Line::Info("carla".into())));
        assert!(!m.is_mention(&Line::Join { nick: "carla".into(), mxid: "@carla:x".into() }));
    }
}
