//! Secret-argument handling for slash commands: the redacted [`Secret`]
//! wrapper and the helpers that mask `/recover` keys on screen and in history.
use std::fmt;

/// A secret typed by the user (recovery key). Never printed: `Debug` is
/// redacted and there is no `Display`.
#[derive(Clone, Eq, PartialEq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Secret(s.into())
    }

    /// The secret itself, for handing to the SDK only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Commands whose arguments are secrets: masked in history and on screen.
pub const SECRET_COMMANDS: &[&str] = &["recover"];

/// If `line` is `/recover ...` (any case), the byte offset where the secret
/// argument starts (after the command word and one separator).
pub fn secret_offset(line: &str) -> Option<usize> {
    let rest = line.strip_prefix('/')?;
    let lead = rest.len() - rest.trim_start().len();
    let rest = &rest[lead..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    if !SECRET_COMMANDS.contains(&rest[..end].to_lowercase().as_str()) {
        return None;
    }
    let sep = rest[end..].chars().next()?;
    Some(1 + lead + end + sep.len_utf8())
}

/// For the screen while typing: every char of the secret becomes `*`.
pub fn mask_secret(line: &str) -> String {
    match secret_offset(line) {
        Some(off) => format!("{}{}", &line[..off], "*".repeat(line[off..].chars().count())),
        None => line.to_string(),
    }
}

/// For history: the secret becomes a fixed `********` (no length leak).
pub fn redact_secret(line: &str) -> String {
    match secret_offset(line) {
        Some(off) => format!("{}********", &line[..off]),
        None => line.to_string(),
    }
}
