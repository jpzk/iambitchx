//! `tracing` layer that turns selected matrix-sdk spans/events (HTTP sends,
//! OAuth refresh, room-key crypto logs) into redacted `DBG` lines.
//!
//! Watches a handful of callsites only (everything else gets
//! `Interest::never`, so the tap costs nothing on unrelated logs):
//!
//! * matrix_sdk `send` spans (http_client): method/path/status/time/size
//! * matrix_sdk `refresh_access_token` span + its warnings (OAuth refresh)
//! * our own token_refresh / session_guard events (proactive refresh)
//! * matrix_sdk_crypto INFO events about room keys
//!
//! Only the `message` and the named span fields are read; `uri` goes through
//! `redact_path`, every text through `redact_text` in `emit`.

use super::redact::{redact_path, truncate};
use super::{Cat, emit, on};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::filter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

const T_HTTP: &str = "matrix_sdk::http_client";
const T_OAUTH: &str = "matrix_sdk::authentication::oauth";
const T_CRYPTO: &str = "matrix_sdk_crypto";
const T_APP_AUTH: [&str; 2] = ["iambitchx::token_refresh", "iambitchx::session_guard"];

fn relevant(m: &Metadata<'_>) -> bool {
    let t = m.target();
    let lvl = *m.level();
    if m.is_span() {
        (m.name() == "send" && t.starts_with(T_HTTP))
            || (m.name() == "refresh_access_token" && t.starts_with(T_OAUTH))
    } else {
        (t.starts_with(T_OAUTH) && lvl <= Level::WARN)
            || (T_APP_AUTH.iter().any(|p| t.starts_with(p)) && lvl <= Level::INFO)
            || (t.starts_with(T_CRYPTO) && lvl <= Level::INFO)
    }
}

/// The layer to stack on the registry in `setup_logging`. Carries its own
/// per-layer filter, so it never changes what the file logger records.
pub(crate) fn layer<S>() -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let f = filter::dynamic_filter_fn(|m: &Metadata<'_>, _cx: &Context<'_, S>| on() && relevant(m))
        .with_callsite_filter(|m| {
            if relevant(m) {
                Interest::sometimes()
            } else {
                Interest::never()
            }
        });
    Tap.with_filter(f)
}

struct Tap;

/// Fields of a matrix_sdk `send` span.
#[derive(Default)]
struct SendFields {
    method: String,
    uri: String,
    status: Option<u64>,
    size: String,
    dur: String,
}

impl SendFields {
    fn set(&mut self, name: &str, v: String) {
        match name {
            "method" => self.method = v,
            "uri" => self.uri = v,
            "response_size" => self.size = v.replace(' ', ""),
            "request_duration" => self.dur = short_duration(&v),
            "status" => self.status = v.parse().ok(),
            _ => {},
        }
    }

    fn line(&self) -> (Cat, String) {
        let path = redact_path(&self.uri);
        let cat = http_cat(&path);
        let status = self.status.map(|s| s.to_string()).unwrap_or_else(|| "ERR".into());
        let text = [self.method.as_str(), &path, &status, &self.dur, &self.size]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        (cat, text)
    }
}

const SEND_FIELDS: [&str; 5] = [
    "method",
    "uri",
    "response_size",
    "request_duration",
    "status",
];

impl Visit for SendFields {
    fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
        // `config` and friends are never formatted
        if SEND_FIELDS.contains(&f.name()) {
            self.set(f.name(), format!("{v:?}"));
        }
    }

    fn record_str(&mut self, f: &Field, v: &str) {
        if SEND_FIELDS.contains(&f.name()) {
            self.set(f.name(), v.to_owned());
        }
    }

    fn record_u64(&mut self, f: &Field, v: u64) {
        if f.name() == "status" {
            self.status = Some(v);
        }
    }

    fn record_i64(&mut self, f: &Field, v: i64) {
        if f.name() == "status" {
            self.status = u64::try_from(v).ok();
        }
    }
}

/// OAuth refresh span state.
#[derive(Default)]
struct RefreshState {
    failed: bool,
}

/// Only the `message` field of an event.
#[derive(Default)]
struct Msg(String);

impl Visit for Msg {
    fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
        if f.name() == "message" {
            self.0 = format!("{v:?}");
        }
    }

    fn record_str(&mut self, f: &Field, v: &str) {
        if f.name() == "message" {
            self.0 = v.to_owned();
        }
    }
}

/// Which category an HTTP request belongs to (by path).
pub(super) fn http_cat(path: &str) -> Cat {
    const CRYPTO: [&str; 4] = [
        "/keys/",
        "/sendToDevice/",
        "/room_keys/",
        "/dehydrated_device",
    ];
    const AUTH: [&str; 5] = ["/oauth2/", "/token", "/login", "/refresh", "/logout"];
    if CRYPTO.iter().any(|p| path.contains(p)) {
        Cat::Crypto
    } else if AUTH.iter().any(|p| path.contains(p)) {
        Cat::Auth
    } else {
        Cat::Http
    }
}

/// `Debug` of a `Duration` (`1.203456s`, `152.3ms`, `850µs`) -> `1.2s`,
/// `152ms`, `<1ms`.
pub(super) fn short_duration(s: &str) -> String {
    let i = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (n, unit) = s.split_at(i);
    let Ok(v) = n.parse::<f64>() else {
        return s.to_string();
    };
    match unit {
        "s" => format!("{v:.1}s"),
        "ms" => format!("{v:.0}ms"),
        "µs" | "us" | "ns" => "<1ms".into(),
        _ => s.to_string(),
    }
}

/// Crypto INFO messages worth a line (room keys / backups).
fn crypto_line(msg: &str) -> Option<String> {
    let l = msg.to_ascii_lowercase();
    if l.contains("room key") || l.contains("megolm") || l.contains("backup") {
        Some(truncate(&l, 120))
    } else {
        None
    }
}

impl<S> Layer<S> for Tap
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let m = attrs.metadata();
        if m.name() == "send" && m.target().starts_with(T_HTTP) {
            let mut f = SendFields::default();
            attrs.record(&mut f);
            span.extensions_mut().insert(f);
        } else if m.target().starts_with(T_OAUTH) {
            span.extensions_mut().insert(RefreshState::default());
            emit(Cat::Auth, None, "oauth token refresh started");
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        if let Some(f) = span.extensions_mut().get_mut::<SendFields>() {
            values.record(f);
        }
    }

    fn on_event(&self, ev: &Event<'_>, ctx: Context<'_, S>) {
        let m = ev.metadata();
        let t = m.target();
        let mut msg = Msg::default();
        ev.record(&mut msg);
        if t.starts_with(T_OAUTH) {
            if let Some(span) = ctx.event_span(ev)
                && let Some(r) = span.extensions_mut().get_mut::<RefreshState>()
            {
                r.failed = true;
            }
            emit(Cat::Auth, None, format!("oauth: {}", truncate(&msg.0, 160)));
        } else if T_APP_AUTH.iter().any(|p| t.starts_with(p)) {
            let text = msg.0.strip_prefix("token_refresh: ").unwrap_or(&msg.0);
            emit(Cat::Auth, None, truncate(text, 160));
        } else if t.starts_with(T_CRYPTO)
            && let Some(l) = crypto_line(&msg.0)
        {
            emit(Cat::Crypto, None, l);
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let ext = span.extensions();
        if let Some(f) = ext.get::<SendFields>() {
            let (cat, text) = f.line();
            emit(cat, None, text);
        } else if let Some(r) = ext.get::<RefreshState>() {
            let text = if r.failed {
                "oauth token refresh failed"
            } else {
                "oauth token refreshed ok"
            };
            emit(Cat::Auth, None, text);
        }
    }
}
