//! # End-to-end encryption UX helpers
//!
//! Pure text for the E2EE `-:-` lines: SAS emoji, decryption failures,
//! in-room `m.key.verification.*` events, the `/encrypt` confirmation.
use matrix_sdk::deserialized_responses::UnableToDecryptReason;

use crate::bitchx::model::Line;

/// The `/encrypt` question. Encryption can never be turned off again.
pub fn encrypt_prompt(room: &str) -> String {
    format!(
        "Enable end-to-end encryption in {room}? This is IRREVERSIBLE: \
         encryption can never be turned off again. [y/n]"
    )
}

/// The SAS block: a header, the 7 emoji with their names, and the hint.
pub fn sas_lines(peer: &str, emojis: &[(String, String)]) -> Vec<Line> {
    let mut out = vec![Line::Info(format!(
        "Verification with {peer}: compare these emoji with the other device"
    ))];
    out.extend(emojis.iter().enumerate().map(|(i, (emoji, name))| {
        Line::Sas { n: i + 1, emoji: emoji.clone(), name: name.clone() }
    }));
    out.push(Line::Info(format!(
        "They match? /verify confirm {peer} -- if not: /verify mismatch {peer}"
    )));
    out
}

/// A short, human reason for a decryption failure.
pub fn utd_reason(reason: &UnableToDecryptReason) -> String {
    match reason {
        UnableToDecryptReason::MissingMegolmSession { withheld_code: Some(code) } => {
            format!("key withheld: {code}")
        },
        UnableToDecryptReason::MissingMegolmSession { withheld_code: None } => {
            "missing room key".into()
        },
        UnableToDecryptReason::UnknownMegolmMessageIndex => "unknown message index".into(),
        UnableToDecryptReason::MalformedEncryptedEvent => "malformed event".into(),
        UnableToDecryptReason::MegolmDecryptionFailure => "decryption failure".into(),
        UnableToDecryptReason::PayloadDeserializationFailure => "bad payload".into(),
        UnableToDecryptReason::MismatchedIdentityKeys => "mismatched identity keys".into(),
        UnableToDecryptReason::SenderIdentityNotTrusted(level) => {
            format!("sender not trusted: {level}")
        },
        _ => "unknown reason".into(),
    }
}

/// `-:-` text for an in-room `m.key.verification.<kind>` event (or the
/// `m.key.verification.request` message type).
pub fn verification_event_text(nick: &str, kind: &str) -> String {
    match kind {
        "request" => format!("{nick} requested key verification"),
        "ready" => format!("{nick} is ready to verify"),
        "start" => format!("{nick} started emoji verification"),
        "accept" => format!("{nick} accepted the verification"),
        "key" => format!("{nick} sent verification keys"),
        "mac" => format!("{nick} confirmed the emoji"),
        "done" => format!("{nick} finished key verification"),
        "cancel" => format!("{nick} cancelled key verification"),
        other => format!("{nick} sent m.key.verification.{other}"),
    }
}

#[cfg(test)]
mod tests {
    use matrix_sdk::deserialized_responses::WithheldCode;
    use ratatui::text::Line as TLine;

    use super::*;
    use crate::bitchx::format::format_line;

    fn text(l: &TLine<'_>) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn sample() -> Vec<(String, String)> {
        [
            ("🐶", "Dog"),
            ("🔑", "Key"),
            ("🎅", "Santa"),
            ("🌍", "Globe"),
            ("📕", "Book"),
            ("⏰", "Clock"),
            ("🔒", "Lock"),
        ]
        .iter()
        .map(|(e, n)| (e.to_string(), n.to_string()))
        .collect()
    }

    #[test]
    fn test_sas_emoji_lines() {
        let lines = sas_lines("armin", &sample());
        assert_eq!(lines.len(), 9);
        assert_eq!(lines[1], Line::Sas { n: 1, emoji: "🐶".into(), name: "Dog".into() });
        assert_eq!(lines[7], Line::Sas { n: 7, emoji: "🔒".into(), name: "Lock".into() });
        let rendered: Vec<String> =
            lines.iter().flat_map(|l| format_line(l, "#r")).map(|l| text(&l)).collect();
        assert_eq!(rendered[1], " SAS 1/7  🐶  Dog");
        assert_eq!(rendered[4], " SAS 4/7  🌍  Globe");
        assert_eq!(rendered[7], " SAS 7/7  🔒  Lock");
        assert!(rendered[0].starts_with(" Verification with armin"));
        assert!(rendered[8].contains("/verify confirm armin"));
        assert!(rendered[8].contains("/verify mismatch armin"));
        assert!(rendered.iter().all(|l| l.starts_with(" ")));
    }

    #[test]
    fn test_utd_lines() {
        let r = UnableToDecryptReason::MissingMegolmSession { withheld_code: None };
        assert_eq!(utd_reason(&r), "missing room key");
        let r = UnableToDecryptReason::MissingMegolmSession {
            withheld_code: Some(WithheldCode::Unverified),
        };
        assert!(utd_reason(&r).starts_with("key withheld: "));
        let l = Line::Undecryptable {
            nick: "armin".into(),
            reason: "missing room key".into(),
        };
        let r = format_line(&l, "#r");
        assert_eq!(text(&r[0]), " [unable to decrypt: missing room key] from armin");
    }

    #[test]
    fn test_verification_event_text() {
        assert_eq!(verification_event_text("bob", "request"), "bob requested key verification");
        assert_eq!(verification_event_text("bob", "cancel"), "bob cancelled key verification");
        assert_eq!(verification_event_text("bob", "zzz"), "bob sent m.key.verification.zzz");
        let r = format_line(&Line::Info(verification_event_text("bob", "done")), "#r");
        assert_eq!(text(&r[0]), " bob finished key verification");
    }

    #[test]
    fn test_encrypt_prompt_says_irreversible() {
        let p = encrypt_prompt("#room");
        assert!(p.contains("#room") && p.contains("IRREVERSIBLE") && p.ends_with("[y/n]"));
    }
}
