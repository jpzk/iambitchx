//! Tests for the /encrypt confirmation and SAS verification flow.

use super::*;

use crate::bitchx::model::{ReqDevice, VerifyAction};

#[test]
fn test_b1_encrypt_prompt_names_room_id_not_spoofable_name() {
    let mut app = app();
    // The real #rust:matrix.org and a room merely *named* like it (F29).
    app.handle_mx(MxEvent::Room(room("!real:matrix.org", "#rust:matrix.org")));
    app.handle_mx(MxEvent::Room(room("!fake:evil.org", "#rust:matrix.org")));
    app.handle_key(alt('6'));
    assert_eq!(app.current_room(), Some(owned_room_id!("!fake:evil.org")));
    assert_eq!(typed(&mut app, "/encrypt"), vec![]);
    let Line::Info(q) = last(&app) else {
        panic!("{:?}", last(&app))
    };
    // The question names the room actually being encrypted, by ID, and
    // never presents the lookalike name as the channel.
    assert!(q.contains("[room id !fake:evil.org]"), "{q}");
    assert!(!q.contains("#rust:matrix.org"), "{q}");
    assert_eq!(app.confirm, Some(Confirm::Encrypt(owned_room_id!("!fake:evil.org"))));
}

#[test]
fn test_encrypt_confirm_flow() {
    let mut app = app();
    app.handle_key(alt('2'));
    let a = owned_room_id!("!a:example.org");
    // Asking does nothing yet; the question says it is irreversible.
    assert_eq!(typed(&mut app, "/encrypt"), vec![]);
    assert!(
        matches!(last(&app), Line::Info(t) if t.contains("IRREVERSIBLE") && t.contains("[y/n]"))
    );
    assert_eq!(app.confirm, Some(Confirm::Encrypt(a.clone())));
    // "n" declines and is not sent as a message.
    assert_eq!(typed(&mut app, "n"), vec![]);
    assert_eq!(app.confirm, None);
    assert_eq!(last(&app), Line::Info("Encryption not enabled".into()));
    // Anything but y/yes declines.
    typed(&mut app, "/encrypt");
    assert_eq!(typed(&mut app, "sure"), vec![]);
    // "y" enables.
    typed(&mut app, "/encrypt");
    assert_eq!(typed(&mut app, "Y"), vec![Effect::EnableEncryption(a.clone())]);
    typed(&mut app, "/encrypt");
    assert_eq!(typed(&mut app, "yes"), vec![Effect::EnableEncryption(a)]);
    // After answering, text is sent normally again.
    assert!(matches!(&typed(&mut app, "hi")[0], Effect::Send { .. }));
    // Not in the status window; not twice in an encrypted room.
    app.handle_key(alt('1'));
    assert_eq!(typed(&mut app, "/encrypt"), vec![]);
    assert!(app.confirm.is_none());
    app.handle_key(alt('2'));
    let mut snap = room("!a:example.org", "#room");
    snap.encrypted = true;
    app.handle_mx(MxEvent::Room(snap));
    assert_eq!(typed(&mut app, "/encrypt"), vec![]);
    assert!(app.confirm.is_none());
    assert!(matches!(last(&app), Line::Info(t) if t.contains("already encrypted")));
}

#[test]
fn test_status_plus_e() {
    let mut app = app();
    app.handle_key(alt('2'));
    assert_eq!(app.status().modes, "");
    let mut snap = room("!a:example.org", "#room");
    snap.encrypted = true;
    app.handle_mx(MxEvent::Room(snap.clone()));
    assert_eq!(app.status().modes, "e");
    snap.invite_only = true;
    app.handle_mx(MxEvent::Room(snap));
    assert_eq!(app.status().modes, "ie");
    let text: String = app.status().line1().spans.iter().map(|s| s.content.to_string()).collect();
    assert!(text.contains("#room(+ie)"), "{text}");
}

/// F37: a dead sync loop turns the status bar `[SYNC LOST]` for good and
/// leaves a -:- line; other room-less errors do not.
#[test]
fn test_f37_sync_death_marker() {
    let mut app = app();
    let bar = |app: &App| -> String {
        app.status().line2().spans.iter().map(|s| s.content.to_string()).collect()
    };
    assert!(bar(&app).contains("[Lag ??]"), "{}", bar(&app));
    app.handle_mx(MxEvent::Info { room_id: None, line: Line::Error("other".into()) });
    assert!(!app.sync_lost);
    app.handle_mx(MxEvent::Info {
        room_id: None,
        line: Line::Error("sync: connection reset".into()),
    });
    assert!(app.sync_lost);
    assert!(bar(&app).contains("[SYNC LOST]"), "{}", bar(&app));
    assert!(!bar(&app).contains("Lag"), "{}", bar(&app));
    let warns = |app: &App| {
        app.windows[0]
            .lines
            .iter()
            .filter(
                |l| matches!(l, Line::Info(t) if t.contains("Sync with the homeserver stopped")),
            )
            .count()
    };
    assert_eq!(warns(&app), 1);
    // A lag sample does not hide it; a second error does not repeat it.
    app.handle_mx(MxEvent::Lag(0));
    app.handle_mx(MxEvent::Info {
        room_id: None,
        line: Line::Error("sync: again".into()),
    });
    assert!(bar(&app).contains("[SYNC LOST]"));
    assert_eq!(warns(&app), 1);
}

/// F44: a second flow with the same user no longer replaces the first;
/// commands then need the flow id, and emoji that cannot be tied to one
/// flow can never be confirmed.
#[test]
fn test_f44_verification_keyed_by_flow() {
    let mut app = app();
    let me2 = owned_user_id!("@armin:remote.tld");
    let req = |flow: &str, dev: &str| MxEvent::Verify {
        user: me2.clone(),
        update: VerifyUpdate::Requested {
            flow_id: flow.into(),
            device: Some(ReqDevice::Known { id: dev.into(), ed25519: None }),
            room_id: None,
            we_started: false,
            identity: false,
        },
    };
    app.handle_mx(req("flowA", "DEVA"));
    app.handle_mx(req("flowB", "DEVB"));
    assert_eq!(app.verifications.len(), 2);
    assert!(
        app.windows[0]
            .lines
            .iter()
            .any(|l| matches!(l, Line::Error(t) if t.contains("another verification pending")))
    );
    // No flow id: refused, both listed.
    for cmd in [
        "/verify accept armin",
        "/verify confirm armin",
        "/verify cancel armin",
    ] {
        assert_eq!(typed(&mut app, cmd), vec![], "{cmd}");
        assert!(
            matches!(last(&app), Line::Error(t)
            if t.contains("2 pending verifications") && t.contains("flowA [DEVA]")
                && t.contains("flowB [DEVB]")),
            "{cmd}"
        );
    }
    // The flow id (or a unique prefix) picks exactly that flow.
    assert_eq!(
        typed(&mut app, "/verify accept armin flowA"),
        vec![Effect::Verify {
            user: me2.clone(),
            flow_id: Some("flowA".into()),
            action: VerifyAction::Accept,
        }]
    );
    assert_eq!(
        typed(&mut app, "/verify accept armin flowB")[0],
        Effect::Verify {
            user: me2.clone(),
            flow_id: Some("flowB".into()),
            action: VerifyAction::Accept,
        }
    );
    assert_eq!(typed(&mut app, "/verify accept armin flowC"), vec![]);
    // Emoji arrive while two flows are pending: they cannot be tied to a
    // flow, so confirm is refused even with an explicit flow id.
    let emojis: Vec<(String, String)> =
        (0..7).map(|i| (format!("e{i}"), format!("n{i}"))).collect();
    app.handle_mx(MxEvent::Verify {
        user: me2.clone(),
        update: VerifyUpdate::Emoji(emojis),
    });
    assert!(app.verifications.iter().all(|v| v.ambiguous));
    assert_eq!(typed(&mut app, "/verify confirm armin flowA"), vec![]);
    assert!(matches!(last(&app), Line::Error(t) if t.contains("may belong to another flow")));
    // Cancelling one: the Matrix side's Cancelled removes that flow only.
    assert_eq!(
        typed(&mut app, "/verify cancel armin flowB")[0],
        Effect::Verify {
            user: me2.clone(),
            flow_id: Some("flowB".into()),
            action: VerifyAction::Cancel,
        }
    );
    app.handle_mx(MxEvent::Verify {
        user: me2.clone(),
        update: VerifyUpdate::Cancelled("user".into()),
    });
    assert_eq!(app.verifications.len(), 1);
    assert_eq!(app.verifications[0].flow_id, "flowA");
    // flowA stays tainted: its emoji were ambiguous.
    assert_eq!(typed(&mut app, "/verify confirm armin"), vec![]);
    // A single fresh flow confirms normally.
    app.handle_mx(MxEvent::Verify {
        user: me2.clone(),
        update: VerifyUpdate::Cancelled("user".into()),
    });
    assert!(app.verifications.is_empty());
    app.handle_mx(req("flowC", "DEVA"));
    assert_eq!(
        typed(&mut app, "/verify confirm armin"),
        vec![Effect::Verify {
            user: me2,
            flow_id: Some("flowC".into()),
            action: VerifyAction::Confirm,
        }]
    );
}

#[test]
fn test_verify_dispatch() {
    let mut app = app();
    let armin = owned_user_id!("@armin:remote.tld");
    // Nothing pending.
    assert_eq!(typed(&mut app, "/verify"), vec![]);
    assert_eq!(last(&app), Line::Info("No pending verifications".into()));
    assert_eq!(typed(&mut app, "/verify accept armin"), vec![]);
    assert!(matches!(last(&app), Line::Error(t) if t.contains("No pending verification")));
    // Start needs no pending request.
    assert_eq!(
        typed(&mut app, "/verify start armin DEV1"),
        vec![Effect::Verify {
            user: armin.clone(),
            flow_id: None,
            action: VerifyAction::Start { device: Some("DEV1".into()) },
        }]
    );
    // An incoming to-device request shows an info line in the status window.
    app.handle_mx(MxEvent::Verify {
        user: armin.clone(),
        update: VerifyUpdate::Requested {
            flow_id: "f1".into(),
            device: Some(ReqDevice::Known { id: "DEV1".into(), ed25519: None }),
            room_id: None,
            we_started: false,
            identity: false,
        },
    });
    assert!(matches!(app.windows[0].lines.last(), Some(Line::Info(t))
        if t.contains("Verification request from @armin:remote.tld [device DEV1 (no ed25519 key), from their device list]")
            && t.contains("/verify accept")));
    typed(&mut app, "/verify");
    assert!(
        matches!(last(&app), Line::Info(t) if t.contains("from @armin:remote.tld [DEV1]: requested"))
    );
    let fx = |action| {
        vec![Effect::Verify {
            user: armin.clone(),
            flow_id: Some("f1".into()),
            action,
        }]
    };
    assert_eq!(typed(&mut app, "/verify accept armin"), fx(VerifyAction::Accept));
    assert_eq!(typed(&mut app, "/verify confirm @armin:remote.tld"), fx(VerifyAction::Confirm));
    assert_eq!(typed(&mut app, "/verify mismatch armin"), fx(VerifyAction::Mismatch));
    assert_eq!(typed(&mut app, "/verify cancel armin"), fx(VerifyAction::Cancel));
    assert!(matches!(
        &typed(&mut app, "/verify start armin")[0],
        Effect::Verify {
            flow_id: Some(_),
            action: VerifyAction::Start { device: None },
            ..
        }
    ));
    assert_eq!(typed(&mut app, "/verify accept nobody"), vec![]);

    // Emoji go to the request's window and the current one.
    app.handle_key(alt('3'));
    let emojis: Vec<(String, String)> =
        (0..7).map(|i| (format!("e{i}"), format!("n{i}"))).collect();
    app.handle_mx(MxEvent::Verify {
        user: armin.clone(),
        update: VerifyUpdate::Emoji(emojis),
    });
    assert_eq!(app.verifications[0].state, "emoji");
    let sas = |w: &Window| w.lines.iter().filter(|l| matches!(l, Line::Sas { .. })).count();
    assert_eq!(sas(&app.windows[0]), 7);
    assert_eq!(sas(app.cur()), 7);
    app.handle_mx(MxEvent::Verify { user: armin.clone(), update: VerifyUpdate::Done });
    assert!(app.verifications.is_empty());
    assert_eq!(last(&app), Line::Info("@armin:remote.tld is now verified (device DEV1)".into()));

    // In-room request lands in that room's window; cancel clears it.
    app.handle_key(alt('1'));
    app.handle_mx(MxEvent::Verify {
        user: armin.clone(),
        update: VerifyUpdate::Requested {
            flow_id: "f2".into(),
            device: None,
            room_id: Some(owned_room_id!("!c:example.org")),
            we_started: false,
            identity: false,
        },
    });
    assert!(
        matches!(app.windows[3].lines.last(), Some(Line::Info(t)) if t.contains("Verification request from"))
    );
    assert!(app.windows[3].unread);
    app.handle_mx(MxEvent::Verify {
        user: armin,
        update: VerifyUpdate::Cancelled("mismatch".into()),
    });
    assert!(app.verifications.is_empty());
    assert!(
        matches!(app.windows[3].lines.last(), Some(Line::Error(t)) if t.contains("cancelled: mismatch"))
    );
}

#[test]
fn test_devices_trust_recover_dispatch() {
    let mut app = app();
    assert_eq!(
        typed(&mut app, "/devices"),
        vec![Effect::Devices(owned_user_id!("@carla:example.org"))]
    );
    assert_eq!(
        typed(&mut app, "/devices armin"),
        vec![Effect::Devices(owned_user_id!("@armin:remote.tld"))]
    );
    assert_eq!(typed(&mut app, "/trust"), vec![Effect::Trust]);
    let key = "EsTc 9xQ2 kLm7 Zz11";
    let fx = typed(&mut app, &format!("/recover {key}"));
    assert_eq!(fx, vec![Effect::Recover(crate::bitchx::commands::Secret::new(key))]);
    // The key is in no scrollback line of any window, nor in Debug output.
    for w in &app.windows {
        for l in &w.lines {
            let s = format!("{l:?}");
            assert!(!s.contains("EsTc") && !s.contains("kLm7"), "{s}");
        }
    }
    assert!(!format!("{fx:?}").contains("EsTc"));
    // A missing key is an error that echoes nothing secret.
    typed(&mut app, "/recover");
    assert!(matches!(last(&app), Line::Error(t) if t.contains("missing argument")));
}

/// E5: with several pending flows `/verify start` must not pick one itself.
#[test]
fn test_e5_verify_start_refuses_to_guess_flow() {
    let mut app = app();
    let armin = owned_user_id!("@armin:remote.tld");
    let req = |flow: &str, dev: &str| MxEvent::Verify {
        user: armin.clone(),
        update: VerifyUpdate::Requested {
            flow_id: flow.into(),
            device: Some(ReqDevice::Known { id: dev.into(), ed25519: None }),
            room_id: None,
            we_started: true,
            identity: false,
        },
    };
    app.handle_mx(req("flowA", "DEVA"));
    app.handle_mx(req("flowB", "DEVB"));
    // No selection: refused, flows listed.
    assert_eq!(typed(&mut app, "/verify start armin"), vec![]);
    assert!(matches!(last(&app), Line::Error(t)
        if t.contains("2 pending verifications") && t.contains("flowA") && t.contains("flowB")));
    let start = |flow: &str, device: Option<&str>| {
        vec![Effect::Verify {
            user: armin.clone(),
            flow_id: Some(flow.into()),
            action: VerifyAction::Start { device: device.map(Into::into) },
        }]
    };
    // Selected by flow id (prefix) or by the device of the flow.
    assert_eq!(typed(&mut app, "/verify start armin flowA"), start("flowA", None));
    assert_eq!(typed(&mut app, "/verify start armin DEVB"), start("flowB", Some("DEVB")));
    // A device with no pending flow is a new request.
    assert_eq!(
        typed(&mut app, "/verify start armin DEVC"),
        vec![Effect::Verify {
            user: armin.clone(),
            flow_id: None,
            action: VerifyAction::Start { device: Some("DEVC".into()) },
        }]
    );
    // An ambiguous prefix is refused.
    assert_eq!(typed(&mut app, "/verify start armin flow"), vec![]);
}

/// E2: an emoji (SAS) flow the peer started is offered, not accepted; the
/// user's `/verify accept` on that flow is what accepts it.
#[test]
fn test_e2_peer_sas_offered_needs_accept() {
    let mut app = app();
    let armin = owned_user_id!("@armin:remote.tld");
    app.handle_mx(MxEvent::Verify {
        user: armin.clone(),
        update: VerifyUpdate::Requested {
            flow_id: "flowA".into(),
            device: Some(ReqDevice::Known { id: "DEVA".into(), ed25519: None }),
            room_id: None,
            we_started: false,
            identity: false,
        },
    });
    app.handle_mx(MxEvent::Verify {
        user: armin.clone(),
        update: VerifyUpdate::SasOffered,
    });
    assert_eq!(app.verifications[0].state, "emoji offered");
    assert!(matches!(last(&app), Line::Info(t)
        if t.contains("started emoji verification") && t.contains("/verify accept")));
    assert_eq!(
        typed(&mut app, "/verify accept armin"),
        vec![Effect::Verify {
            user: armin,
            flow_id: Some("flowA".into()),
            action: VerifyAction::Accept,
        }]
    );
}

/// E3: the device shown with a request is the SDK-resolved one (id + key,
/// labelled as from the device list); a sender-chosen id the SDK does not
/// know is flagged and never matched; a repeated flow id cannot replace it.
#[test]
fn test_e3_request_shows_sdk_device_not_claim() {
    let mut app = app();
    let armin = owned_user_id!("@armin:remote.tld");
    let req = |flow: &str, device: ReqDevice| MxEvent::Verify {
        user: armin.clone(),
        update: VerifyUpdate::Requested {
            flow_id: flow.into(),
            device: Some(device),
            room_id: None,
            we_started: false,
            identity: false,
        },
    };
    app.handle_mx(req(
        "flowA",
        ReqDevice::Known {
            id: "DEVA".into(),
            ed25519: Some("AbCdEfGh".into()),
        },
    ));
    assert!(matches!(last(&app), Line::Info(t)
        if t.contains("[device DEVA (ed25519 AbCd EfGh), from their device list]")));
    assert_eq!(app.verifications[0].device.as_deref(), Some("DEVA"));
    // Same flow id, other device: the first (the one the SDK keeps) stays.
    app.handle_mx(req("flowA", ReqDevice::Unknown("EVIL".into())));
    assert!(matches!(last(&app), Line::Error(t) if t.contains("Ignored a repeated")));
    assert_eq!(app.verifications.len(), 1);
    assert_eq!(app.verifications[0].device.as_deref(), Some("DEVA"));
    // A device the SDK does not know: flagged, not recorded as the device.
    app.handle_mx(req("flowB", ReqDevice::Unknown("EVIL\u{1b}[2J".into())));
    let shown = app.windows[0].lines.iter().any(|l| {
        matches!(l, Line::Info(t) if t.contains("UNKNOWN device") && t.contains("sender's claim only")
            && !t.contains('\u{1b}'))
    });
    assert!(shown);
    assert_eq!(app.verifications[1].device, None);
}

/// E4: a flow we started names what it verifies -- one device (request,
/// emoji header, done line) or the whole cross-signing identity, flagged as
/// trusting ALL their devices -- so the two are never shown the same.
#[test]
fn test_e4_flow_names_device_or_identity() {
    let mut app = app();
    let armin = owned_user_id!("@armin:remote.tld");
    let ev = |update| MxEvent::Verify { user: armin.clone(), update };
    let emojis: Vec<(String, String)> =
        (0..7).map(|i| (format!("e{i}"), format!("n{i}"))).collect();
    let info = |app: &App, needle: &str| {
        app.windows[0]
            .lines
            .iter()
            .any(|l| matches!(l, Line::Info(t) if t.contains(needle)))
    };

    // /verify start armin DEVX: the device stays shown through to done.
    app.handle_mx(ev(VerifyUpdate::Requested {
        flow_id: "fd".into(),
        device: Some(ReqDevice::Known { id: "DEVX".into(), ed25519: Some("AbCd".into()) }),
        room_id: None,
        we_started: true,
        identity: false,
    }));
    assert!(info(&app, "Verification request sent to @armin:remote.tld [device DEVX"));
    assert!(!info(&app, "IDENTITY"));
    app.handle_mx(ev(VerifyUpdate::Emoji(emojis.clone())));
    assert!(info(&app, "Verification with @armin:remote.tld (device DEVX): compare"));
    app.handle_mx(ev(VerifyUpdate::Done));
    assert_eq!(last(&app), Line::Info("@armin:remote.tld is now verified (device DEVX)".into()));

    // /verify start armin (cross-signing identity): flagged as ALL devices.
    app.handle_mx(ev(VerifyUpdate::Requested {
        flow_id: "fi".into(),
        device: None,
        room_id: None,
        we_started: true,
        identity: true,
    }));
    assert!(info(&app, "[IDENTITY: verifies ALL their devices]"));
    app.handle_mx(ev(VerifyUpdate::Emoji(emojis)));
    assert!(info(&app, "(their cross-signing identity: ALL their devices): compare"));
    app.handle_mx(ev(VerifyUpdate::Done));
    assert_eq!(
        last(&app),
        Line::Info(
            "@armin:remote.tld is now verified (their cross-signing identity: ALL their devices)"
                .into()
        )
    );
}
