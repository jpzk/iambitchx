//! Unit tests for the debug tap (redaction, line formatting, layer filter).

use super::layer::*;
use super::redact::*;
use super::sync::*;
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
    let g = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let entered = s.enter();
        s.record(
            "uri",
            format!("https://hs.example/_matrix/client/v3/sync?since=s1&access_token={AT}"),
        );
        s.record("method", tracing::field::debug(http_method()));
        s.record("status", 200u16);
        s.record("response_size", "4.3 kB");
        s.record("request_duration", tracing::field::debug(std::time::Duration::from_millis(1234)));
        drop(entered);
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
    let texts: Vec<String> = lines.iter().map(|l| format!("{} {}", l.cat.name(), l.text)).collect();
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
    pub(super) struct Method;
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
    assert_eq!(redact_path("/_matrix/client/v3/keys/query?x=1#f"), "/_matrix/client/v3/keys/query");
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
        "expires_in_ms": 300_000,
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
