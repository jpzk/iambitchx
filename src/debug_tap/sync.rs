//! `/sync` hook: `on_sync` is called once per `/sync` response from the
//! sync loop (one hook line in matrix.rs). The SDK-facing part only pulls
//! raw JSON / ids out of the response; the `*_line` helpers are pure and
//! unit-tested.

use std::fmt::Write as _;

use super::redact::{MASK_STR, compact_json, short_id, truncate};
use super::{Cat, emit, on, raw, start_forwarder, wants};
use serde_json::Value;

/// One `/sync` response -> sync / event / presence / ephemeral / todevice /
/// crypto lines. Returns at once when debug is off.
pub(crate) fn on_sync(resp: &matrix_sdk::sync::SyncResponse, tx: &crate::matrix::Tx) {
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
                    TimelineEventKind::PlainText { .. } => Enc::Plain,
                };
                let v = raw_value(tev.raw().json().get());
                emit(Cat::Event, room, event_line(&v, enc));
                if raw() {
                    emit(Cat::Event, room, raw_line(&v));
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
                emit(Cat::Presence, None, raw_line(&v));
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
            emit(Cat::ToDevice, None, raw_line(&v));
        }
    }
}

/// `/set debug_raw on`: max chars of the compact raw JSON.
pub(super) const RAW_MAX: usize = 300;

/// Free-text fields a user types (message bodies, status messages): masked
/// in raw output by default (F-05), wherever they are nested.
const FREE_TEXT_KEYS: &[&str] = &["body", "formatted_body", "status_msg"];

/// `raw {…}`: the compact redacted JSON with free-text fields masked.
pub(super) fn raw_line(v: &Value) -> String {
    let mut v = v.clone();
    mask_free_text(&mut v);
    format!("raw {}", compact_json(&v, RAW_MAX))
}

fn mask_free_text(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if FREE_TEXT_KEYS.contains(&k.as_str()) {
                    *x = Value::String(MASK_STR.into());
                } else {
                    mask_free_text(x);
                }
            }
        },
        Value::Array(a) => a.iter_mut().for_each(mask_free_text),
        _ => {},
    }
}

fn raw_value(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
}

/// Encryption state of an event as the client saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Enc {
    Plain,
    Decrypted,
    Utd,
}

/// Counters of one `/sync` response.
pub(super) struct SyncCounts<'a> {
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
pub(super) fn sync_line(c: &SyncCounts<'_>) -> String {
    let mut s = format!("next_batch={} joined={}", short_id(c.next_batch, 8), c.joined);
    if c.invited > 0 {
        let _ = write!(s, " invited={}", c.invited);
    }
    if c.left > 0 {
        let _ = write!(s, " left={}", c.left);
    }
    let _ = write!(
        s,
        " events={} presence={} ephemeral={} to_device={}",
        c.events, c.presence, c.ephemeral, c.to_device
    );
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
pub(super) fn event_line(v: &Value, enc: Enc) -> String {
    let ty = str_at(v, &["type"]).unwrap_or("?");
    let id = str_at(v, &["event_id"]).map(|i| short_id(i, 9)).unwrap_or_default();
    let from = str_at(v, &["sender"]).unwrap_or("?");
    let mut s = ty.to_string();
    if let Some(sk) = str_at(v, &["state_key"]).filter(|k| !k.is_empty()) {
        let _ = write!(s, " [{sk}]");
    }
    if !id.is_empty() {
        s.push(' ');
        s.push_str(&id);
    }
    let _ = write!(s, " from {from}");
    let alg = str_at(v, &["content", "algorithm"]).unwrap_or("m.megolm.v1.aes-sha2");
    match enc {
        Enc::Plain => {},
        Enc::Decrypted => s.push_str(" (m.megolm.v1.aes-sha2 -> decrypted)"),
        Enc::Utd => {
            let _ = write!(s, " ({alg} -> UTD)");
        },
    }
    s
}

/// `@armin unavailable last_active 4h` (+ `active`, `"status"`).
pub(super) fn presence_line(v: &Value) -> Option<String> {
    let user = str_at(v, &["sender"])?;
    let c = v.get("content")?;
    let state = c.get("presence").and_then(Value::as_str).unwrap_or("?");
    let mut s = format!("{user} {state}");
    if let Some(ms) = c.get("last_active_ago").and_then(Value::as_u64) {
        let _ = write!(s, " last_active {}", ago(ms));
    }
    if c.get("currently_active").and_then(Value::as_bool) == Some(true) {
        s.push_str(" active");
    }
    if let Some(m) = c.get("status_msg").and_then(Value::as_str).filter(|m| !m.is_empty()) {
        let _ = write!(s, " \"{}\"", truncate(m, 40));
    }
    Some(s)
}

/// Milliseconds -> `12s` / `5m` / `4h` / `2d`.
pub(super) fn ago(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

/// Receipts / typing: `typing @a, @b` / `receipt m.read @u -> $ev..`.
pub(super) fn ephemeral_lines(v: &Value) -> Vec<String> {
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
pub(super) fn to_device_line(v: &Value, enc: Enc) -> String {
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
pub(super) fn room_key_line(v: &Value, enc: Enc) -> Option<String> {
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
