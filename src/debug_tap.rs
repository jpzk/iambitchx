//! Protocol debug tap: turns under-the-hood Matrix activity into short
//! `DBG` lines for the BitchX UI (`/set debug on`).
//!
//! Cost when off: one relaxed `AtomicBool` load per hook. Lines travel over a
//! bounded channel with `try_send` — the tap never blocks and drops on full.
//!
//! SECURITY: everything that leaves this module goes through the redaction
//! helpers below: query strings dropped, `Authorization` never read, any
//! token/password/secret/key-ish JSON field replaced, and bare `mat_`/`mar_`/
//! `syt_` token strings masked wherever they appear.
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::filter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// Debug categories, same names/order as `bitchx::vars::DEBUG_CATEGORIES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cat {
    Http = 0,
    Sync = 1,
    Event = 2,
    Auth = 3,
    Crypto = 4,
    Presence = 5,
    Ephemeral = 6,
    ToDevice = 7,
}

impl Cat {
    pub const ALL: [Cat; 8] = [
        Cat::Http,
        Cat::Sync,
        Cat::Event,
        Cat::Auth,
        Cat::Crypto,
        Cat::Presence,
        Cat::Ephemeral,
        Cat::ToDevice,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Cat::Http => "http",
            Cat::Sync => "sync",
            Cat::Event => "event",
            Cat::Auth => "auth",
            Cat::Crypto => "crypto",
            Cat::Presence => "presence",
            Cat::Ephemeral => "ephemeral",
            Cat::ToDevice => "todevice",
        }
    }

    pub fn from_name(s: &str) -> Option<Cat> {
        Cat::ALL.into_iter().find(|c| c.name() == s)
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// One line for the UI. `room` = room id for room-scoped lines (rendered in
/// that window), `None` = status window. `text` is already redacted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbgLine {
    pub cat: Cat,
    pub room: Option<String>,
    pub text: String,
}

const CHANNEL_CAP: usize = 512;

static ENABLED: AtomicBool = AtomicBool::new(false);
static MASK: AtomicU8 = AtomicU8::new(u8::MAX);
static RAW: AtomicBool = AtomicBool::new(false);
type Chan = (SyncSender<DbgLine>, Mutex<Option<Receiver<DbgLine>>>);
static CHAN: OnceLock<Chan> = OnceLock::new();

/// The bounded channel, created on first use (lines emitted before the UI
/// forwarder runs wait in it, up to `CHANNEL_CAP`).
fn chan() -> &'static Chan {
    CHAN.get_or_init(|| {
        let (tx, rx) = sync_channel(CHANNEL_CAP);
        (tx, Mutex::new(Some(rx)))
    })
}

/// Take the receiver (once); `None` afterwards.
pub fn init() -> Option<Receiver<DbgLine>> {
    chan().1.lock().ok()?.take()
}

/// Start (once) the thread that moves `DbgLine`s into the UI channel as
/// `MxEvent::Debug`. A plain thread, not a tokio task: it blocks on the
/// receiver and must never hold up runtime shutdown on `/quit`.
pub fn start_forwarder(tx: &crate::matrix::Tx) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let Some(rx) = init() else { return };
    let tx = tx.clone();
    let spawned = std::thread::Builder::new().name("debug_tap".into()).spawn(move || {
        while let Ok(line) = rx.recv() {
            if tx.send(crate::bitchx::model::MxEvent::Debug(line)).is_err() {
                break;
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!("debug_tap: no forwarder thread: {e}");
    }
}

/// Apply `/set debug`, `/set debug_filter`, `/set debug_raw`.
pub fn configure(on: bool, filter: &str, raw: bool) {
    MASK.store(filter_mask(filter), Ordering::Relaxed);
    RAW.store(raw, Ordering::Relaxed);
    ENABLED.store(on, Ordering::Relaxed);
}

/// Mask for a normalized `debug_filter` value (`all` or comma list).
pub fn filter_mask(filter: &str) -> u8 {
    let mut m = 0u8;
    for w in filter.split([',', ' ']).map(str::trim).filter(|w| !w.is_empty()) {
        let w = w.to_ascii_lowercase();
        if w == "all" || w == "*" {
            return u8::MAX;
        }
        if let Some(c) = Cat::from_name(&w) {
            m |= c.bit();
        }
    }
    if m == 0 { u8::MAX } else { m }
}

/// Cheap gate: is debug on at all?
#[inline]
pub fn on() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Is debug on and this category selected?
#[inline]
pub fn wants(cat: Cat) -> bool {
    on() && MASK.load(Ordering::Relaxed) & cat.bit() != 0
}

pub fn raw() -> bool {
    RAW.load(Ordering::Relaxed)
}

/// Send a line (redacted again defensively). Never blocks; drops when the
/// channel is full or not initialised.
pub fn emit(cat: Cat, room: Option<&str>, text: impl AsRef<str>) {
    if !wants(cat) {
        return;
    }
    let tx = &chan().0;
    let line = DbgLine {
        cat,
        room: room.map(str::to_owned),
        text: redact_text(text.as_ref()),
    };
    match tx.try_send(line) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {},
    }
}

// ------------------------------------------------------------ tracing layer
//
// Watches a handful of callsites only (everything else gets
// `Interest::never`, so the tap costs nothing on unrelated logs):
//   * matrix_sdk `send` spans (http_client): method/path/status/time/size
//   * matrix_sdk `refresh_access_token` span + its warnings (OAuth refresh)
//   * our own token_refresh / session_guard events (proactive refresh)
//   * matrix_sdk_crypto INFO events about room keys
// Only the `message` and the named span fields are read; `uri` goes through
// `redact_path`, every text through `redact_text` in `emit`.

const T_HTTP: &str = "matrix_sdk::http_client";
const T_OAUTH: &str = "matrix_sdk::authentication::oauth";
const T_CRYPTO: &str = "matrix_sdk_crypto";
const T_APP_AUTH: [&str; 2] = ["iambitchx::token_refresh", "iambitchx::session_guard"];

fn relevant(m: &Metadata<'_>) -> bool {
    let t = m.target();
    let lvl = *m.level();
    if m.is_span() {
        (m.name() == "send" && t.starts_with(T_HTTP)) ||
            (m.name() == "refresh_access_token" && t.starts_with(T_OAUTH))
    } else {
        (t.starts_with(T_OAUTH) && lvl <= Level::WARN) ||
            (T_APP_AUTH.iter().any(|p| t.starts_with(p)) && lvl <= Level::INFO) ||
            (t.starts_with(T_CRYPTO) && lvl <= Level::INFO)
    }
}

/// The layer to stack on the registry in `setup_logging`. Carries its own
/// per-layer filter, so it never changes what the file logger records.
pub fn layer<S>() -> impl Layer<S>
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
pub fn http_cat(path: &str) -> Cat {
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
pub fn short_duration(s: &str) -> String {
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
            if let Some(span) = ctx.event_span(ev) &&
                let Some(r) = span.extensions_mut().get_mut::<RefreshState>()
            {
                r.failed = true;
            }
            emit(Cat::Auth, None, format!("oauth: {}", truncate(&msg.0, 160)));
        } else if T_APP_AUTH.iter().any(|p| t.starts_with(p)) {
            let text = msg.0.strip_prefix("token_refresh: ").unwrap_or(&msg.0);
            emit(Cat::Auth, None, truncate(text, 160));
        } else if t.starts_with(T_CRYPTO) &&
            let Some(l) = crypto_line(&msg.0)
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

// ---------------------------------------------------------------- redaction

const MASK_STR: &str = "<redacted>";

/// URL or path -> path only (scheme/host/query/fragment dropped).
pub fn redact_path(url: &str) -> String {
    let s = url.split(['?', '#']).next().unwrap_or("");
    let path = match s.find("://") {
        Some(i) => {
            let rest = &s[i + 3..];
            rest.find('/').map(|j| &rest[j..]).unwrap_or("/")
        },
        None => s,
    };
    redact_text(path)
}

/// Field names whose values must never be shown.
pub fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
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
    NEEDLES.iter().any(|n| k.contains(n)) || k == "code" || k == "device_keys"
}

/// Mask bare Matrix token strings (`mat_…`, `mar_…`, `syt_…`) and
/// `Bearer …` values anywhere in free text.
pub fn redact_text(s: &str) -> String {
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
pub fn redact_json(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            Value::Object(
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
            )
        },
        Value::Array(a) => Value::Array(a.iter().map(redact_json).collect()),
        Value::String(s) => Value::String(redact_text(s)),
        other => other.clone(),
    }
}

/// Compact redacted JSON, truncated to `max` chars (with `…`).
pub fn compact_json(v: &Value, max: usize) -> String {
    let s = serde_json::to_string(&redact_json(v)).unwrap_or_default();
    truncate(&s, max)
}

/// Truncate to `max` chars, appending `…` when cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// Shorten an opaque id like `s123_456_…` or `$abcdef…` to `head..`.
pub fn short_id(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}..", s.chars().take(n).collect::<String>())
    }
}

// ------------------------------------------------------------- sync hook
//
// `on_sync` is called once per `/sync` response from the sync loop (one hook
// line in matrix.rs). The SDK-facing part only pulls raw JSON / ids out of
// the response; the `*_line` helpers below are pure and unit-tested.

/// One `/sync` response -> sync / event / presence / ephemeral / todevice /
/// crypto lines. Returns at once when debug is off.
pub fn on_sync(resp: &matrix_sdk::sync::SyncResponse, tx: &crate::matrix::Tx) {
    use matrix_sdk::deserialized_responses::{ProcessedToDeviceEvent, TimelineEventKind};
    start_forwarder(tx);
    if !on() {
        return;
    }
    let events: usize = resp.rooms.joined.values().map(|u| u.timeline.events.len()).sum();
    let ephemeral: usize = resp.rooms.joined.values().map(|u| u.ephemeral.len()).sum();
    let counts = SyncCounts {
        next_batch: &resp.next_batch,
        joined: resp.rooms.joined.len(),
        invited: resp.rooms.invited.len(),
        left: resp.rooms.left.len(),
        events,
        presence: resp.presence.len(),
        ephemeral,
        to_device: resp.to_device.len(),
    };
    emit(Cat::Sync, None, sync_line(&counts));

    for (room_id, update) in &resp.rooms.joined {
        let room = Some(room_id.as_str());
        if wants(Cat::Event) {
            for tev in &update.timeline.events {
                let enc = match &tev.kind {
                    TimelineEventKind::Decrypted(_) => Enc::Decrypted,
                    TimelineEventKind::UnableToDecrypt { .. } => Enc::Utd,
                    _ => Enc::Plain,
                };
                let v = raw_value(tev.raw().json().get());
                emit(Cat::Event, room, event_line(&v, enc));
                if raw() {
                    emit(Cat::Event, room, format!("raw {}", compact_json(&v, RAW_MAX)));
                }
            }
        }
        if wants(Cat::Ephemeral) {
            for e in &update.ephemeral {
                let v = raw_value(e.json().get());
                for l in ephemeral_lines(&v) {
                    emit(Cat::Ephemeral, room, l);
                }
            }
        }
    }
    if wants(Cat::Presence) {
        for p in &resp.presence {
            let v = raw_value(p.json().get());
            if let Some(l) = presence_line(&v) {
                emit(Cat::Presence, None, l);
            }
            if raw() {
                emit(Cat::Presence, None, format!("raw {}", compact_json(&v, RAW_MAX)));
            }
        }
    }
    for td in &resp.to_device {
        let (enc, v) = match td {
            ProcessedToDeviceEvent::Decrypted { raw, .. } => {
                (Enc::Decrypted, raw_value(raw.json().get()))
            },
            ProcessedToDeviceEvent::UnableToDecrypt { encrypted_event, .. } => {
                (Enc::Utd, raw_value(encrypted_event.json().get()))
            },
            other => (Enc::Plain, raw_value(other.as_raw().json().get())),
        };
        emit(Cat::ToDevice, None, to_device_line(&v, enc));
        if let Some(l) = room_key_line(&v, enc) {
            emit(Cat::Crypto, None, l);
        }
        if raw() {
            emit(Cat::ToDevice, None, format!("raw {}", compact_json(&v, RAW_MAX)));
        }
    }
}

/// `/set debug_raw on`: max chars of the compact raw JSON.
pub const RAW_MAX: usize = 300;

fn raw_value(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
}

/// Encryption state of an event as the client saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enc {
    Plain,
    Decrypted,
    Utd,
}

/// Counters of one `/sync` response.
pub struct SyncCounts<'a> {
    pub next_batch: &'a str,
    pub joined: usize,
    pub invited: usize,
    pub left: usize,
    pub events: usize,
    pub presence: usize,
    pub ephemeral: usize,
    pub to_device: usize,
}

/// `next_batch=s123.. joined=3 events=5 presence=2 ephemeral=1 to_device=0`
pub fn sync_line(c: &SyncCounts<'_>) -> String {
    let mut s = format!("next_batch={} joined={}", short_id(c.next_batch, 8), c.joined);
    if c.invited > 0 {
        s.push_str(&format!(" invited={}", c.invited));
    }
    if c.left > 0 {
        s.push_str(&format!(" left={}", c.left));
    }
    s.push_str(&format!(
        " events={} presence={} ephemeral={} to_device={}",
        c.events, c.presence, c.ephemeral, c.to_device
    ));
    s
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for p in path {
        cur = cur.get(p)?;
    }
    cur.as_str()
}

/// `m.room.message $abcdefgh.. from @x:hs (m.megolm.v1.aes-sha2 -> decrypted)`
pub fn event_line(v: &Value, enc: Enc) -> String {
    let ty = str_at(v, &["type"]).unwrap_or("?");
    let id = str_at(v, &["event_id"]).map(|i| short_id(i, 9)).unwrap_or_default();
    let from = str_at(v, &["sender"]).unwrap_or("?");
    let mut s = ty.to_string();
    if let Some(sk) = str_at(v, &["state_key"]).filter(|k| !k.is_empty()) {
        s.push_str(&format!(" [{sk}]"));
    }
    if !id.is_empty() {
        s.push(' ');
        s.push_str(&id);
    }
    s.push_str(&format!(" from {from}"));
    let alg = str_at(v, &["content", "algorithm"]).unwrap_or("m.megolm.v1.aes-sha2");
    match enc {
        Enc::Plain => {},
        Enc::Decrypted => s.push_str(" (m.megolm.v1.aes-sha2 -> decrypted)"),
        Enc::Utd => s.push_str(&format!(" ({alg} -> UTD)")),
    }
    s
}

/// `@armin unavailable last_active 4h` (+ `active`, `"status"`).
pub fn presence_line(v: &Value) -> Option<String> {
    let user = str_at(v, &["sender"])?;
    let c = v.get("content")?;
    let state = c.get("presence").and_then(Value::as_str).unwrap_or("?");
    let mut s = format!("{user} {state}");
    if let Some(ms) = c.get("last_active_ago").and_then(Value::as_u64) {
        s.push_str(&format!(" last_active {}", ago(ms)));
    }
    if c.get("currently_active").and_then(Value::as_bool) == Some(true) {
        s.push_str(" active");
    }
    if let Some(m) = c.get("status_msg").and_then(Value::as_str).filter(|m| !m.is_empty()) {
        s.push_str(&format!(" \"{}\"", truncate(m, 40)));
    }
    Some(s)
}

/// Milliseconds -> `12s` / `5m` / `4h` / `2d`.
pub fn ago(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

/// Receipts / typing: `typing @a, @b` / `receipt m.read @u -> $ev..`.
pub fn ephemeral_lines(v: &Value) -> Vec<String> {
    let ty = str_at(v, &["type"]).unwrap_or("?");
    let Some(c) = v.get("content") else {
        return vec![ty.to_string()];
    };
    match ty {
        "m.typing" => {
            let users: Vec<&str> = c
                .get("user_ids")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if users.is_empty() {
                vec!["typing (nobody)".into()]
            } else {
                vec![format!("typing {}", users.join(", "))]
            }
        },
        "m.receipt" => {
            let mut out = vec![];
            for (ev, kinds) in c.as_object().into_iter().flatten() {
                for (kind, users) in kinds.as_object().into_iter().flatten() {
                    for user in users.as_object().into_iter().flatten().map(|(u, _)| u) {
                        out.push(format!("receipt {kind} {user} -> {}", short_id(ev, 9)));
                    }
                }
            }
            out
        },
        other => vec![other.to_string()],
    }
}

/// `m.room_key from @x:hs (olm -> decrypted)`
pub fn to_device_line(v: &Value, enc: Enc) -> String {
    let ty = str_at(v, &["type"]).unwrap_or("?");
    let from = str_at(v, &["sender"]).unwrap_or("?");
    let tail = match enc {
        Enc::Plain => "",
        Enc::Decrypted => " (olm -> decrypted)",
        Enc::Utd => " (olm -> UTD)",
    };
    format!("{ty} from {from}{tail}")
}

/// Decrypted `m.room_key` / `m.forwarded_room_key` ->
/// `room key received for !room from @x:hs`.
pub fn room_key_line(v: &Value, enc: Enc) -> Option<String> {
    let ty = str_at(v, &["type"])?;
    if enc != Enc::Decrypted || !matches!(ty, "m.room_key" | "m.forwarded_room_key") {
        return None;
    }
    let room = str_at(v, &["content", "room_id"]).unwrap_or("?");
    let from = str_at(v, &["sender"]).unwrap_or("?");
    let fwd = if ty == "m.forwarded_room_key" {
        " (forwarded)"
    } else {
        ""
    };
    Some(format!("room key received for {room} from {from}{fwd}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    const AT: &str = "mat_AbCdEf0123456789xyzQWE_1";
    const RT: &str = "mar_ZyXwVu9876543210abcDEF_2";
    const SYT: &str = "syt_ZWxpYQ_aBcDeFgHiJkLmNoPqRsT_0a1b2c";

    /// The tap is process-global: tests that toggle it hold this guard
    /// (serialises them) and get the drained shared receiver.
    pub(crate) fn harness() -> std::sync::MutexGuard<'static, Receiver<DbgLine>> {
        static RX: OnceLock<std::sync::Mutex<Receiver<DbgLine>>> = OnceLock::new();
        let m = RX.get_or_init(|| std::sync::Mutex::new(init().expect("tap init")));
        let g = m.lock().unwrap_or_else(|e| e.into_inner());
        while g.try_recv().is_ok() {}
        g
    }

    fn drain(rx: &Receiver<DbgLine>) -> Vec<DbgLine> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    #[test]
    fn test_layer_http_auth_crypto() {
        use tracing::field::Empty;
        use tracing_subscriber::layer::SubscriberExt;

        let rx = harness();
        let sub = tracing_subscriber::registry().with(layer());
        let run = || {
            let s = tracing::info_span!(target: "matrix_sdk::http_client", "send",
                uri = Empty, config = "cfg", method = Empty, status = Empty,
                response_size = Empty, request_duration = Empty);
            let _e = s.enter();
            s.record(
                "uri",
                format!("https://hs.example/_matrix/client/v3/sync?since=s1&access_token={AT}"),
            );
            s.record("method", tracing::field::debug(http_method()));
            s.record("status", 200u16);
            s.record("response_size", "4.3 kB");
            s.record(
                "request_duration",
                tracing::field::debug(std::time::Duration::from_millis(1234)),
            );
            drop(_e);
            drop(s);
            let k = tracing::info_span!(target: "matrix_sdk::http_client", "send",
                uri = "https://hs.example/_matrix/client/v3/keys/query", method = "POST",
                status = 200u64, response_size = Empty, request_duration = Empty);
            drop(k);
            {
                let r = tracing::info_span!(target: "matrix_sdk::authentication::oauth",
                    "refresh_access_token");
                let _e = r.enter();
            }
            tracing::info!(target: "iambitchx::token_refresh",
                "token_refresh: next proactive refresh in 240s (ttl 300s, margin 60s)");
            tracing::info!(target: "iambitchx::token_refresh",
                "token_refresh: proactive refresh done");
            tracing::info!(target: "matrix_sdk_crypto::store", "Received a new megolm room key");
            tracing::info!(target: "matrix_sdk_crypto::store", "unrelated chatter");
            tracing::info!(target: "some_other_crate", "noise {RT}");
        };

        configure(false, "all", false);
        tracing::subscriber::with_default(sub, || {
            run();
            assert!(rx.try_recv().is_err(), "nothing while off");
            configure(true, "all", false);
            run();
        });
        configure(false, "all", false);
        let lines = drain(&rx);
        let texts: Vec<String> =
            lines.iter().map(|l| format!("{} {}", l.cat.name(), l.text)).collect();
        for t in &texts {
            no_secrets(t);
        }
        let want = [
            "http GET /_matrix/client/v3/sync 200 1.2s 4.3kB",
            "crypto POST /_matrix/client/v3/keys/query 200",
            "auth oauth token refresh started",
            "auth oauth token refreshed ok",
            "auth next proactive refresh in 240s (ttl 300s, margin 60s)",
            "auth proactive refresh done",
            "crypto received a new megolm room key",
        ];
        assert_eq!(texts, want);
    }

    fn http_method() -> http_like::Method {
        http_like::Method
    }

    mod http_like {
        /// Debug-prints like `http::Method::GET`.
        pub struct Method;
        impl std::fmt::Debug for Method {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("GET")
            }
        }
    }

    #[test]
    fn test_short_duration_and_cat() {
        assert_eq!(short_duration("1.203456789s"), "1.2s");
        assert_eq!(short_duration("152.345ms"), "152ms");
        assert_eq!(short_duration("850µs"), "<1ms");
        assert_eq!(short_duration("weird"), "weird");
        assert_eq!(http_cat("/_matrix/client/v3/sync"), Cat::Http);
        assert_eq!(http_cat("/_matrix/client/v3/keys/query"), Cat::Crypto);
        assert_eq!(http_cat("/_matrix/client/v3/sendToDevice/m.room_key/1"), Cat::Crypto);
        assert_eq!(http_cat("/oauth2/token"), Cat::Auth);
    }

    fn no_secrets(s: &str) {
        for bad in [
            AT,
            RT,
            SYT,
            "AbCdEf0123",
            "ZyXwVu9876",
            "ZWxpYQ_aBc",
            "hunter2",
            "Bearer mat",
        ] {
            assert!(!s.contains(bad), "leaked {bad:?} in {s:?}");
        }
        assert!(!s.contains('?'), "query string leaked in {s:?}");
    }

    #[test]
    fn test_redact_path_drops_query_and_host() {
        let u = format!(
            "https://matrix.example.org/_matrix/client/v3/sync?since=s1&access_token={AT}&timeout=30000"
        );
        let p = redact_path(&u);
        assert_eq!(p, "/_matrix/client/v3/sync");
        no_secrets(&p);
        assert_eq!(
            redact_path("/_matrix/client/v3/keys/query?x=1#f"),
            "/_matrix/client/v3/keys/query"
        );
        assert_eq!(redact_path("https://h.example"), "/");
    }

    #[test]
    fn test_redact_text_token_strings() {
        let s = format!("token {AT} then {RT} and {SYT}; Authorization: Bearer {AT}");
        let r = redact_text(&s);
        no_secrets(&r);
        assert!(r.contains("mat_<redacted>"));
        assert!(r.contains("mar_<redacted>"));
        assert!(r.contains("syt_<redacted>"));
        assert!(r.contains("Bearer <redacted>"));
        // ordinary words are untouched
        assert_eq!(redact_text("format_x primary_key MAT_"), "format_x primary_key MAT_<redacted>");
        assert_eq!(redact_text("DBG http GET /sync 200"), "DBG http GET /sync 200");
    }

    #[test]
    fn test_redact_json_keys() {
        let v = json!({
            "access_token": AT,
            "refresh_token": RT,
            "expires_in_ms": 300000,
            "password": "hunter2",
            "auth": {"type": "m.login.password", "password": "hunter2"},
            "headers": {"Authorization": format!("Bearer {AT}")},
            "content": {
                "algorithm": "m.megolm.v1.aes-sha2",
                "ciphertext": "AwgAEpABlongciphertext",
                "session_id": "sess",
                "sender_key": "curve",
                "session_key": "megolm",
                "body": format!("look {SYT}")
            },
            "list": [{"token": "abc"}, "syt_inlist_abcdef"],
            "type": "m.room.message"
        });
        let s = compact_json(&v, 10_000);
        no_secrets(&s);
        for k in [
            "AwgAEpABlong",
            "megolm\"",
            "\"curve\"",
            "\"sess\"",
            "abc\"",
            "inlist",
        ] {
            assert!(!s.contains(k), "leaked {k} in {s}");
        }
        assert!(s.contains("m.megolm.v1.aes-sha2"));
        assert!(s.contains("\"type\":\"m.room.message\""));
        assert!(s.contains("300000"), "non-secret numeric kept");
        assert!(!s.contains("token\":\"mat"));
    }

    #[test]
    fn test_compact_json_truncates() {
        let v = json!({"body": "x".repeat(1000)});
        let s = compact_json(&v, 300);
        assert_eq!(s.chars().count(), 300);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn test_filter_mask_and_gate() {
        assert_eq!(filter_mask("all"), u8::MAX);
        assert_eq!(filter_mask(""), u8::MAX);
        assert_eq!(filter_mask("http"), Cat::Http.bit());
        assert_eq!(filter_mask("http,sync"), Cat::Http.bit() | Cat::Sync.bit());
        assert_eq!(filter_mask("todevice, presence"), Cat::ToDevice.bit() | Cat::Presence.bit());
        for c in Cat::ALL {
            assert_eq!(Cat::from_name(c.name()), Some(c));
        }
    }

    #[test]
    fn test_emit_gated_and_redacted() {
        let rx = harness();
        configure(false, "all", false);
        emit(Cat::Http, None, "off line");
        assert!(rx.try_recv().is_err(), "nothing while off");
        configure(true, "http", false);
        emit(Cat::Sync, None, "filtered out");
        emit(Cat::Http, Some("!r:x"), format!("GET /sync {AT}"));
        let l = rx.try_recv().expect("http line");
        assert_eq!(l.cat, Cat::Http);
        assert_eq!(l.room.as_deref(), Some("!r:x"));
        no_secrets(&l.text);
        assert!(rx.try_recv().is_err());
        // never blocks when full
        for _ in 0..CHANNEL_CAP * 2 {
            emit(Cat::Http, None, "x");
        }
        configure(false, "all", false);
    }

    #[test]
    fn test_helpers() {
        assert_eq!(short_id("s123456789", 4), "s123..");
        assert_eq!(short_id("s1", 4), "s1");
    }

    #[test]
    fn test_sync_lines() {
        let c = SyncCounts {
            next_batch: "s123456789_99_0",
            joined: 3,
            invited: 0,
            left: 0,
            events: 5,
            presence: 2,
            ephemeral: 1,
            to_device: 0,
        };
        assert_eq!(
            sync_line(&c),
            "next_batch=s1234567.. joined=3 events=5 presence=2 ephemeral=1 to_device=0"
        );

        let ev = serde_json::json!({"type":"m.room.message","event_id":"$abcdefghijkl",
            "sender":"@x:hs","content":{"body":"hi"}});
        assert_eq!(
            event_line(&ev, Enc::Decrypted),
            "m.room.message $abcdefgh.. from @x:hs (m.megolm.v1.aes-sha2 -> decrypted)"
        );
        let utd = serde_json::json!({"type":"m.room.encrypted","event_id":"$e","sender":"@x:hs",
            "content":{"algorithm":"m.megolm.v1.aes-sha2","ciphertext":"AwgAEpAB",
            "session_id":"SESS","sender_key":"CURVE"}});
        assert_eq!(
            event_line(&utd, Enc::Utd),
            "m.room.encrypted $e from @x:hs (m.megolm.v1.aes-sha2 -> UTD)"
        );
        let raw = compact_json(&utd, RAW_MAX);
        for s in ["AwgAEpAB", "SESS", "CURVE"] {
            assert!(!raw.contains(s), "{raw}");
        }
        let st = serde_json::json!({"type":"m.room.member","state_key":"@y:hs","event_id":"$m","sender":"@y:hs"});
        assert_eq!(event_line(&st, Enc::Plain), "m.room.member [@y:hs] $m from @y:hs");

        let p = serde_json::json!({"type":"m.presence","sender":"@armin:hs",
            "content":{"presence":"unavailable","last_active_ago":14_400_000u64}});
        assert_eq!(presence_line(&p).unwrap(), "@armin:hs unavailable last_active 4h");
        assert_eq!(ago(59_000), "59s");
        assert_eq!(ago(300_000), "5m");
        assert_eq!(ago(2 * 86_400_000), "2d");

        let ty = serde_json::json!({"type":"m.typing","content":{"user_ids":["@a:hs","@b:hs"]}});
        assert_eq!(ephemeral_lines(&ty), vec!["typing @a:hs, @b:hs"]);
        let rc = serde_json::json!({"type":"m.receipt","content":{"$evid12345678":
            {"m.read":{"@u:hs":{"ts":1}}}}});
        assert_eq!(ephemeral_lines(&rc), vec!["receipt m.read @u:hs -> $evid1234.."]);

        let rk = serde_json::json!({"type":"m.room_key","sender":"@x:hs",
            "content":{"room_id":"!r:hs","session_key":"SECRETKEY","session_id":"S"}});
        assert_eq!(to_device_line(&rk, Enc::Decrypted), "m.room_key from @x:hs (olm -> decrypted)");
        assert_eq!(
            room_key_line(&rk, Enc::Decrypted).unwrap(),
            "room key received for !r:hs from @x:hs"
        );
        assert_eq!(room_key_line(&rk, Enc::Plain), None);
        assert!(!compact_json(&rk, RAW_MAX).contains("SECRETKEY"));
    }
}
