//! # End-to-end encryption on the Matrix side
//!
//! Verification handlers (to-device and in-room requests), the SAS emoji
//! flow, `/devices`, `/trust`, `/recover` and `/encrypt`. Progress goes to
//! the UI as [`MxEvent::Verify`] and `-:-` lines. Keys, recovery keys and
//! tokens are never logged or printed.
use futures::StreamExt;
use matrix_sdk::encryption::verification::{
    SasState, SasVerification, Verification, VerificationRequest, VerificationRequestState,
};
use matrix_sdk::ruma::events::key::verification::VerificationMethod;
use matrix_sdk::ruma::events::key::verification::request::ToDeviceKeyVerificationRequestEvent;
use matrix_sdk::ruma::events::room::message::{MessageType, OriginalSyncRoomMessageEvent};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};
use matrix_sdk::{Client, Room};

use crate::bitchx::model::{Line, MxEvent, Secret, VerifyAction, VerifyUpdate};
use crate::matrix::Tx;

fn update(tx: &Tx, user: &UserId, update: VerifyUpdate) {
    let _ = tx.send(MxEvent::Verify { user: user.to_owned(), update });
}

fn info(tx: &Tx, room_id: Option<OwnedRoomId>, text: String) {
    let _ = tx.send(MxEvent::Info { room_id, line: Line::Info(text) });
}

/// Register the handlers for incoming key verification requests:
/// `m.key.verification.request` as a to-device event (device verification)
/// and as an in-room `m.room.message` (user verification, e.g. Element's
/// "Verify user" in a DM).
pub(crate) fn register_verification_handlers(client: &Client, tx: Tx) {
    let to_device_tx = tx.clone();
    client.add_event_handler(move |ev: ToDeviceKeyVerificationRequestEvent, client: Client| {
        let tx = to_device_tx.clone();
        async move {
            let flow_id = ev.content.transaction_id.to_string();
            if let Some(req) =
                client.encryption().get_verification_request(&ev.sender, &flow_id).await
            {
                track_request(req, Some(ev.content.from_device.to_string()), tx);
            }
        }
    });

    client.add_event_handler(move |ev: OriginalSyncRoomMessageEvent, client: Client| {
        let tx = tx.clone();
        async move {
            let MessageType::VerificationRequest(content) = &ev.content.msgtype else {
                return;
            };
            if client.user_id() == Some(&*ev.sender) {
                // Our own request: tracked where it was created.
                return;
            }
            if let Some(req) = client
                .encryption()
                .get_verification_request(&ev.sender, &ev.event_id)
                .await
            {
                track_request(req, Some(content.from_device.to_string()), tx);
            }
        }
    });
}

/// Report a request and follow it: ready -> SAS -> emoji -> done/cancelled.
pub(crate) fn track_request(req: VerificationRequest, device: Option<String>, tx: Tx) {
    let user = req.other_user_id().to_owned();
    update(
        &tx,
        &user,
        VerifyUpdate::Requested {
            flow_id: req.flow_id().to_string(),
            device,
            room_id: req.room_id().map(ToOwned::to_owned),
            we_started: req.we_started(),
        },
    );

    tokio::spawn(async move {
        let mut changes = req.changes();
        let mut state = Some(req.state());
        loop {
            let st = match state.take() {
                Some(s) => s,
                None => match changes.next().await {
                    Some(s) => s,
                    None => break,
                },
            };
            match st {
                VerificationRequestState::Created { .. }
                | VerificationRequestState::Requested { .. } => {},
                VerificationRequestState::Ready { .. } => {
                    update(&tx, &user, VerifyUpdate::Ready);
                    // E6: `Ok(None)` means no SAS is running -- report it, never as success.
                    if req.we_started()
                        && let Err(e) = start_sas(&req).await
                    {
                        update(&tx, &user, VerifyUpdate::Cancelled(e.to_string()));
                        let _ = req.cancel().await;
                        break;
                    }
                },
                VerificationRequestState::Transitioned { verification, .. } => {
                    if let Verification::SasV1(sas) = verification {
                        track_sas(sas, tx.clone());
                    } else {
                        update(
                            &tx,
                            &user,
                            VerifyUpdate::Cancelled("only emoji (SAS) is supported".into()),
                        );
                        let _ = req.cancel().await;
                    }
                    break;
                },
                VerificationRequestState::Done => {
                    update(&tx, &user, VerifyUpdate::Done);
                    break;
                },
                VerificationRequestState::Cancelled(info) => {
                    update(&tx, &user, VerifyUpdate::Cancelled(info.reason().to_string()));
                    break;
                },
            }
        }
    });
}

/// Follow a SAS verification; accepts it if the other side started it.
pub(crate) fn track_sas(sas: SasVerification, tx: Tx) {
    tokio::spawn(async move {
        let user = sas.other_user_id().to_owned();
        if !sas.we_started()
            && let Err(e) = sas.accept().await
        {
            update(&tx, &user, VerifyUpdate::Cancelled(e.to_string()));
            return;
        }
        let mut changes = sas.changes();
        let mut state = Some(sas.state());
        loop {
            let st = match state.take() {
                Some(s) => s,
                None => match changes.next().await {
                    Some(s) => s,
                    None => break,
                },
            };
            match st {
                SasState::KeysExchanged { emojis, decimals } => {
                    let pairs = if let Some(e) = emojis {
                        e.emojis
                            .iter()
                            .map(|e| (e.symbol.to_string(), e.description.to_string()))
                            .collect()
                    } else {
                        let (a, b, c) = decimals;
                        vec![
                            (a.to_string(), "number".into()),
                            (b.to_string(), "number".into()),
                            (c.to_string(), "number".into()),
                        ]
                    };
                    update(&tx, &user, VerifyUpdate::Emoji(pairs));
                },
                SasState::Confirmed => update(&tx, &user, VerifyUpdate::Confirmed),
                SasState::Done { .. } => {
                    update(&tx, &user, VerifyUpdate::Done);
                    break;
                },
                SasState::Cancelled(info) => {
                    update(&tx, &user, VerifyUpdate::Cancelled(info.reason().to_string()));
                    break;
                },
                _ => {},
            }
        }
    });
}

/// E6: `start_sas` answers `Ok(None)` (not an error) when the request is not
/// ready or the other device does not support SAS; that is a failure.
fn sas_started<T>(started: Option<T>) -> anyhow::Result<T> {
    started.ok_or_else(|| {
        anyhow::anyhow!(
            "emoji verification did not start: the request is not ready or the other device does not support emoji (SAS)"
        )
    })
}

async fn start_sas(req: &VerificationRequest) -> anyhow::Result<SasVerification> {
    sas_started(req.start_sas().await?)
}

async fn sas_of(client: &Client, user: &UserId, flow_id: &str) -> anyhow::Result<SasVerification> {
    match client.encryption().get_verification(user, flow_id).await {
        Some(Verification::SasV1(sas)) => Ok(sas),
        _ => Err(anyhow::anyhow!("no emoji verification with {user} yet")),
    }
}

/// `/verify accept|start|confirm|mismatch|cancel`.
pub(crate) async fn verify(
    client: &Client,
    user: OwnedUserId,
    flow_id: Option<String>,
    action: VerifyAction,
    tx: &Tx,
) -> anyhow::Result<()> {
    let enc = client.encryption();
    let req = match &flow_id {
        Some(f) => enc.get_verification_request(&user, f).await,
        None => None,
    };

    match action {
        VerifyAction::Accept => {
            let req = req.ok_or_else(|| anyhow::anyhow!("request from {user} expired"))?;
            req.accept_with_methods(vec![VerificationMethod::SasV1]).await?;
            info(
                tx,
                req.room_id().map(ToOwned::to_owned),
                format!("Accepted verification from {user}"),
            );
        },
        VerifyAction::Start { device } => {
            if let Some(req) =
                req.filter(matrix_sdk::encryption::verification::VerificationRequest::is_ready)
            {
                // The tracker picks up the SAS when the request transitions.
                start_sas(&req).await?;
                return Ok(());
            }
            let req = match device {
                Some(device) => {
                    let dev = enc
                        .get_device(&user, device.as_str().into())
                        .await?
                        .ok_or_else(|| anyhow::anyhow!("{user} has no device {device}"))?;
                    dev.request_verification_with_methods(vec![VerificationMethod::SasV1])
                        .await?
                },
                None if client.user_id() == Some(&*user) => {
                    anyhow::bail!(
                        "give one of your devices: /verify start {user} <device> (see /devices)"
                    );
                },
                None => {
                    let identity = enc
                        .get_user_identity(&user)
                        .await?
                        .ok_or_else(|| anyhow::anyhow!("{user} has no cross-signing identity"))?;
                    identity
                        .request_verification_with_methods(vec![VerificationMethod::SasV1])
                        .await?
                },
            };
            track_request(req, None, tx.clone());
        },
        VerifyAction::Confirm => {
            let f = flow_id.unwrap_or_default();
            sas_of(client, &user, &f).await?.confirm().await?;
        },
        VerifyAction::Mismatch => {
            let f = flow_id.unwrap_or_default();
            sas_of(client, &user, &f).await?.mismatch().await?;
        },
        VerifyAction::Cancel => {
            let f = flow_id.unwrap_or_default();
            match sas_of(client, &user, &f).await {
                Ok(sas) => sas.cancel().await?,
                Err(_) => {
                    req.ok_or_else(|| anyhow::anyhow!("no verification with {user}"))?
                        .cancel()
                        .await?;
                },
            }
        },
    }
    Ok(())
}

/// `/devices <user>`: one `-:-` line per device with its verified flag.
pub(crate) async fn devices(client: &Client, user: OwnedUserId, tx: &Tx) -> anyhow::Result<()> {
    let enc = client.encryption();
    let own_device = client.device_id().map(ToOwned::to_owned);
    let devices = enc.get_user_devices(&user).await?;
    let identity = enc.get_user_identity(&user).await?;
    let who = match &identity {
        Some(i) if i.is_verified() => "verified",
        Some(_) => "unverified",
        None => "no cross-signing identity",
    };
    info(tx, None, format!("Devices of {user} [{who}]"));
    let mut n = 0;
    for d in devices.devices() {
        n += 1;
        let flag = if d.is_verified() {
            "verified"
        } else {
            "UNVERIFIED"
        };
        let me = if Some(d.device_id()) == own_device.as_deref() {
            " (this device)"
        } else {
            ""
        };
        let name = d.display_name().map(crate::bitchx::format::sanitize).unwrap_or_default();
        info(tx, None, format!("  {:<12} [{flag}] {name}{me}", d.device_id()));
    }
    info(tx, None, format!("End of /DEVICES for {user} ({n} devices)"));
    Ok(())
}

/// `/trust`: our own cross-signing / recovery status.
pub(crate) async fn trust(client: &Client, tx: &Tx) -> anyhow::Result<()> {
    let enc = client.encryption();
    let yn = |b: bool| if b { "yes" } else { "NO" };
    match enc.cross_signing_status().await {
        Some(s) => {
            info(
                tx,
                None,
                format!(
                    "Cross-signing: master {} self-signing {} user-signing {}",
                    yn(s.has_master),
                    yn(s.has_self_signing),
                    yn(s.has_user_signing)
                ),
            );
        },
        None => info(tx, None, "Cross-signing: encryption not initialised".into()),
    }
    if let Some(dev) = enc.get_own_device().await? {
        info(
            tx,
            None,
            format!(
                "This device {}: {}",
                dev.device_id(),
                if dev.is_cross_signed_by_owner() {
                    "cross-signed"
                } else {
                    "NOT cross-signed"
                }
            ),
        );
    }
    info(
        tx,
        None,
        format!("Recovery: {:?} -- /recover <recovery key> to import", enc.recovery().state()),
    );
    Ok(())
}

/// `/recover <key>`: import secrets from secure backup, then bootstrap
/// cross-signing if it is still missing. The key is never printed.
pub(crate) async fn recover(client: &Client, key: Secret, tx: &Tx) -> anyhow::Result<()> {
    let enc = client.encryption();
    enc.recovery()
        .recover(key.expose())
        .await
        .map_err(|e| anyhow::anyhow!("recovery failed: {e}"))?;
    drop(key);
    info(tx, None, "Secrets imported from secure backup".into());
    let complete = enc.cross_signing_status().await.is_some_and(|s| s.is_complete());
    if !complete {
        info(tx, None, "Cross-signing missing, bootstrapping...".into());
        enc.bootstrap_cross_signing_if_needed(None).await?;
    }
    trust(client, tx).await
}

/// `/encrypt` (after y): enable megolm in the room. Irreversible.
pub(crate) async fn enable_encryption(room: &Room, tx: &Tx) -> anyhow::Result<()> {
    room.enable_encryption().await?;
    info(tx, Some(room.room_id().to_owned()), "End-to-end encryption enabled (megolm), +e".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_e6_sas_not_started_is_an_error() {
        let err = sas_started::<()>(None).unwrap_err().to_string();
        assert!(err.contains("did not start"), "{err}");
        assert_eq!(sas_started(Some(7)).unwrap(), 7);
    }
}
