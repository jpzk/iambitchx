//! Protocol debug tap: turns under-the-hood Matrix activity into short
//! `DBG` lines for the BitchX UI (`/set debug on`).
//!
//! Cost when off: one relaxed `AtomicBool` load per hook. Lines travel over a
//! bounded channel with `try_send` — the tap never blocks and drops on full.
//!
//! SECURITY: everything that leaves this module goes through the redaction
//! helpers in the `redact` submodule: query strings dropped, `Authorization` never read, any
//! token/password/secret/key-ish JSON field replaced, and bare `mat_`/`mar_`/
//! `syt_` token strings masked wherever they appear.
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Mutex, OnceLock};

use self::redact::redact_text;

mod layer;
mod redact;
mod sync;
#[cfg(test)]
pub(crate) mod tests;

pub use layer::layer;
pub use sync::on_sync;

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
