//! Redaction helpers: everything the tap emits passes through these
//! (query strings dropped, secret-ish JSON fields replaced, bare
//! `mat_`/`mar_`/`syt_` tokens masked) plus small text-shaping utilities.

use serde_json::Value;

const MASK_STR: &str = "<redacted>";

/// URL or path -> path only (scheme/host/query/fragment dropped).
pub(super) fn redact_path(url: &str) -> String {
    let s = url.split(['?', '#']).next().unwrap_or("");
    let path = match s.find("://") {
        Some(i) => {
            let rest = &s[i + 3..];
            rest.find('/').map_or("/", |j| &rest[j..])
        },
        None => s,
    };
    redact_text(path)
}

/// Field names whose values must never be shown.
pub(super) fn is_secret_key(key: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "token",
        "password",
        "secret",
        "authorization",
        "key",
        "ciphertext",
        "session_id",
        "mac",
        "signature",
        "passphrase",
        "recovery",
        "pickle",
        "nonce",
    ];
    let k = key.to_ascii_lowercase();
    NEEDLES.iter().any(|n| k.contains(n)) || k == "code" || k == "device_keys"
}

/// Mask bare Matrix token strings (`mat_…`, `mar_…`, `syt_…`) and
/// `Bearer …` values anywhere in free text.
pub(crate) fn redact_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let at_word_start = i == 0 || !is_tok_byte(b[i - 1]);
        if at_word_start {
            let rest = &s[i..];
            let prefix = ["mat_", "mar_", "syt_"].iter().find(|p| starts_with_ci(rest, p));
            if let Some(p) = prefix {
                out.push_str(&rest[..p.len()]);
                out.push_str(MASK_STR);
                i += p.len();
                while i < b.len() && is_tok_byte(b[i]) {
                    i += 1;
                }
                continue;
            }
            if starts_with_ci(rest, "bearer ") {
                out.push_str("Bearer ");
                out.push_str(MASK_STR);
                i += 7;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'"' {
                    i += 1;
                }
                continue;
            }
        }
        // i < s.len() (loop guard) and i only advances by whole ASCII tokens or
        // ch.len_utf8(), so it is on a char boundary and next() is always Some.
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_tok_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.' || c == b'~'
}

fn starts_with_ci(s: &str, p: &str) -> bool {
    s.len() >= p.len() && s.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes())
}

/// Recursively redact a JSON value: secret-ish keys replaced, strings
/// scrubbed of token patterns.
pub(super) fn redact_json(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| {
                    let v = if is_secret_key(k) {
                        Value::String(MASK_STR.into())
                    } else {
                        redact_json(v)
                    };
                    (k.clone(), v)
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(redact_json).collect()),
        Value::String(s) => Value::String(redact_text(s)),
        other => other.clone(),
    }
}

/// Compact redacted JSON, truncated to `max` chars (with `…`).
pub(super) fn compact_json(v: &Value, max: usize) -> String {
    let s = serde_json::to_string(&redact_json(v)).unwrap_or_default();
    truncate(&s, max)
}

/// Truncate to `max` chars, appending `…` when cut.
pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// Shorten an opaque id like `s123_456_…` or `$abcdef…` to `head..`.
pub(super) fn short_id(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}..", s.chars().take(n).collect::<String>())
    }
}
