//! Redaction helpers: everything the tap emits passes through these
//! (query strings dropped, secret-ish JSON fields replaced, bare
//! tokens and token-shaped runs masked, fail closed) plus small text-shaping
//! utilities.

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

/// Fail-closed free-text redaction. Masks, wherever they occur (no
/// word-start requirement, F-04): Matrix token prefixes (`mat_`, `mar_`,
/// `syt_`, `mct_`, also with a percent-encoded `%5F` underscore) and
/// `Bearer …` values; then any token-shaped run (F-03): JWTs (`eyJ…` or
/// three long dot-separated base64url segments), long base64 / base64url
/// / hex blobs.
pub(crate) fn redact_text(s: &str) -> String {
    mask_shapes(&mask_prefixes(s))
}

const TOKEN_PREFIXES: &[&str] = &["mat", "mar", "syt", "mct"];

/// Length of a `mat_` / `syt%5F` style prefix at the start of `rest`.
fn token_prefix_len(rest: &str) -> Option<usize> {
    let p = TOKEN_PREFIXES.iter().find(|p| starts_with_ci(rest, p))?;
    let tail = &rest[p.len()..];
    if tail.starts_with('_') {
        Some(p.len() + 1)
    } else if starts_with_ci(tail, "%5f") {
        Some(p.len() + 3)
    } else {
        None
    }
}

fn mask_prefixes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let rest = &s[i..];
        if let Some(n) = token_prefix_len(rest) {
            out.push_str(&rest[..n]);
            i += n;
            // already masked: keep idempotent
            if !s[i..].starts_with(MASK_STR) {
                out.push_str(MASK_STR);
            }
            while i < b.len() && (is_tok_byte(b[i]) || b[i] == b'%') {
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
        // i < s.len() (loop guard) and i only advances by whole ASCII tokens or
        // ch.len_utf8(), so it is on a char boundary and next() is always Some.
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Bytes of a base64 / base64url / hex / JWT-ish run.
fn is_run_byte(c: u8) -> bool {
    is_tok_byte(c) || matches!(c, b'+' | b'/' | b'=' | b'%')
}

/// Replace every token-shaped run with [`MASK_STR`].
fn mask_shapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if is_run_byte(b[i]) {
            let start = i;
            while i < b.len() && is_run_byte(b[i]) {
                i += 1;
            }
            // run bytes are ASCII, so start..i is on char boundaries
            let run = &s[start..i];
            out.push_str(if token_shaped(run) { MASK_STR } else { run });
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Does this run look like a credential? Errs on the side of masking.
fn token_shaped(run: &str) -> bool {
    let is_b64url = |seg: &str| {
        seg.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'=')
    };
    let dot_segs: Vec<&str> = run.split('.').collect();
    // JWT: base64url header `{"…` = `eyJ…`, or a.b.c with long segments
    if dot_segs.iter().any(|g| g.starts_with("eyJ") && g.len() >= 10) {
        return true;
    }
    if dot_segs
        .windows(3)
        .any(|w| w.iter().all(|g| g.len() >= 8 && is_b64url(g)))
    {
        return true;
    }
    // long hex (any case, incl. all-digit)
    if run.len() >= 32 && run.bytes().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // a long high-entropy piece (paths, dotted names and `k=v` split apart)
    if run
        .split(['.', '/', '~'])
        .flat_map(assign_parts)
        .any(|p| p.len() >= 16 && char_classes(p) >= 2)
    {
        return true;
    }
    // standard base64 whose `/` or `+` splits it into short pieces
    !run.starts_with('/')
        && run.len() >= 20
        && run.contains(['/', '+'])
        && char_classes(run) >= 2
}

/// Split `k=v` at an `=` that is not base64 padding (followed by more data).
fn assign_parts(p: &str) -> Vec<&str> {
    let b = p.as_bytes();
    let mut parts = vec![];
    let mut start = 0;
    for i in 0..b.len() {
        if b[i] == b'=' && b.get(i + 1).is_some_and(|c| *c != b'=') {
            parts.push(&p[start..i]);
            start = i + 1;
        }
    }
    parts.push(&p[start..]);
    parts
}

/// How many of {lower, upper, digit} occur in `s`.
fn char_classes(s: &str) -> usize {
    let b = s.as_bytes();
    [
        b.iter().any(u8::is_ascii_lowercase),
        b.iter().any(u8::is_ascii_uppercase),
        b.iter().any(u8::is_ascii_digit),
    ]
    .iter()
    .filter(|x| **x)
    .count()
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
