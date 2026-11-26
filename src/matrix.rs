//! # The Matrix side
//!
//! A sync loop on matrix-sdk that turns timeline events into BitchX
//! [`Line`]s, room snapshots for the windows, history backfill, and the
//! executor for the [`Effect`]s the UI asks for. Everything it learns goes
//! to the UI as [`MxEvent`]s over a channel.
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use matrix_sdk::config::SyncSettings;
use matrix_sdk::deserialized_responses::{TimelineEvent, TimelineEventKind};
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::api::client::directory::get_public_rooms_filtered::v3::Request as PublicRoomsRequest;
use matrix_sdk::ruma::api::client::presence::get_presence::v3::Request as GetPresenceRequest;
use matrix_sdk::ruma::api::client::presence::set_presence::v3::Request as PresenceRequest;
use matrix_sdk::ruma::api::client::profile::DisplayName;
use matrix_sdk::ruma::api::error::ErrorKind;
use matrix_sdk::ruma::events::room::member::MembershipChange;
use matrix_sdk::ruma::events::room::message::{MessageType, RoomMessageEventContent};
use matrix_sdk::ruma::events::room::power_levels::UserPowerLevel;
use matrix_sdk::ruma::events::{
    AnySyncMessageLikeEvent,
    AnySyncStateEvent,
    AnySyncTimelineEvent,
    SyncMessageLikeEvent,
    SyncStateEvent,
};
use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::ruma::room::JoinRuleKind;
use matrix_sdk::ruma::room::RoomType;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{
    Int,
    OwnedRoomId,
    OwnedRoomOrAliasId,
    OwnedServerName,
    OwnedUserId,
    UserId,
    uint,
};
use matrix_sdk::{Client, LoopCtrl, Room, RoomMemberships};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch;

use crate::bitchx::channels::{LIST_MAX, ListArgs, ListEntry, not_found_hint};
use crate::bitchx::e2ee::{utd_reason, verification_event_text};
use crate::bitchx::format::sanitize;
use crate::bitchx::model::{
    Effect,
    Line,
    Member,
    MxEvent,
    RoomSnapshot,
    Stamped,
    away_of,
    nick_of,
};
use crate::config::ApplicationSettings;
use crate::login;
use crate::session_guard::Mode;

pub type Tx = UnboundedSender<MxEvent>;

/// Our own away state as the sync loop sees it (`true` while away, by hand or
/// automatic). Every `/sync` carries `set_presence`, and Synapse applies it on
/// each request: with the default (`online`) the loop would undo `/away`
/// within one long-poll. [`Effect::Away`] flips this; the loop rebuilds its
/// [`SyncSettings`] after the current response.
static SYNC_AWAY: LazyLock<watch::Sender<bool>> = LazyLock::new(|| watch::Sender::new(false));

/// The `set_presence` for `/sync`: `unavailable` while away (manual `/away`
/// or AUTO_AWAY), `online` otherwise.
pub fn sync_presence(away: bool) -> PresenceState {
    if away {
        PresenceState::Unavailable
    } else {
        PresenceState::Online
    }
}

/// The explicit `PUT /presence` for an away change: away sets `unavailable`
/// with the reason as `status_msg`; back (`/back`, auto-back) sets `online`
/// and clears `status_msg` (field absent, stored by Synapse as no status).
/// The display name is never touched by away.
pub fn away_presence(reason: Option<&str>) -> (PresenceState, Option<String>) {
    (sync_presence(reason.is_some()), reason.map(str::to_string))
}

/// Sync settings for the current away state.
fn sync_settings(away: bool) -> SyncSettings {
    SyncSettings::default()
        .timeout(Duration::from_secs(30))
        .set_presence(sync_presence(away))
}

fn power(pl: UserPowerLevel) -> i64 {
    match pl {
        UserPowerLevel::Infinite => i64::MAX,
        UserPowerLevel::Int(i) => i.into(),
        _ => 0,
    }
}

/// `#alias` for rooms with a canonical alias (the server part dropped when it
/// is ours), else `#Room_name`; DMs are named after the peer.
fn channel_name(room: &Room, me: &UserId, peer_nick: Option<String>) -> String {
    if let Some(alias) = room.canonical_alias() {
        return if alias.server_name() == me.server_name() {
            format!("#{}", alias.alias())
        } else {
            alias.to_string()
        };
    }
    if let Some(nick) = peer_nick {
        return nick;
    }
    let name = room
        .cached_display_name()
        .map(|n| n.to_string())
        .unwrap_or_else(|| room.room_id().to_string());
    let name: String = sanitize(&name).split_whitespace().collect::<Vec<_>>().join("_");
    if name.starts_with('#') || name.starts_with('!') {
        name
    } else {
        format!("#{name}")
    }
}

/// Everything a window needs to know about a room.
pub async fn snapshot(room: &Room, me: &UserId) -> RoomSnapshot {
    let members: Vec<Member> = room
        .members_no_sync(RoomMemberships::JOIN)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|m| Member::new(m.user_id().to_owned(), power(m.power_level())))
        .collect();

    let is_direct = room.is_direct().await.unwrap_or(false);
    let dm_peer = if is_direct {
        room.direct_targets()
            .iter()
            .filter_map(|t| t.as_user_id().map(ToOwned::to_owned))
            .find(|u| u != me)
            .or_else(|| members.iter().map(|m| m.user_id.clone()).find(|u| u != me))
    } else {
        None
    };
    let peer_nick = dm_peer.as_deref().map(nick_of);

    RoomSnapshot {
        room_id: Some(room.room_id().to_owned()),
        name: channel_name(room, me, peer_nick),
        dm_peer,
        topic: room.topic().filter(|t| !t.trim().is_empty()).map(|t| sanitize(&t)),
        encrypted: room.encryption_state().is_encrypted(),
        invite_only: room
            .join_rule()
            .is_some_and(|r| matches!(r.kind(), JoinRuleKind::Invite | JoinRuleKind::Knock)),
        members,
    }
}

fn state_lines(ev: AnySyncStateEvent) -> Vec<Line> {
    let sender = nick_of(ev.sender());

    match ev {
        AnySyncStateEvent::RoomMember(SyncStateEvent::Original(m)) => {
            let Ok(target) = UserId::parse(m.state_key.as_str()) else {
                return vec![];
            };
            let mxid = target.to_string();
            let victim = nick_of(&target);
            let reason = m.content.reason.clone().filter(|r| !r.trim().is_empty());

            match m.membership_change() {
                MembershipChange::Joined |
                MembershipChange::InvitationAccepted |
                MembershipChange::KnockAccepted => vec![Line::Join { nick: victim, mxid }],
                MembershipChange::Left |
                MembershipChange::InvitationRejected |
                MembershipChange::KnockRetracted => vec![Line::Part { nick: victim, mxid, reason }],
                MembershipChange::Kicked => vec![Line::Kick { victim, kicker: sender, reason }],
                MembershipChange::KickedAndBanned => {
                    vec![
                        Line::Mode {
                            modes: format!("+b {victim}"),
                            nick: sender.clone(),
                        },
                        Line::Kick { victim, kicker: sender, reason },
                    ]
                },
                MembershipChange::Banned => {
                    vec![Line::Mode { modes: format!("+b {victim}"), nick: sender }]
                },
                MembershipChange::Unbanned => {
                    vec![Line::Mode { modes: format!("-b {victim}"), nick: sender }]
                },
                MembershipChange::Invited => vec![Line::Invite { nick: sender, target: mxid }],
                MembershipChange::ProfileChanged { displayname_change: Some(c), .. } => {
                    let name = c.new.filter(|n| !n.trim().is_empty()).map(sanitize);
                    vec![Line::DisplayName { nick: victim, name }]
                },
                _ => vec![],
            }
        },
        AnySyncStateEvent::RoomTopic(SyncStateEvent::Original(t)) => {
            vec![Line::Topic {
                nick: Some(sender),
                topic: sanitize(&t.content.topic),
            }]
        },
        AnySyncStateEvent::RoomName(SyncStateEvent::Original(n)) => {
            vec![Line::Info(format!(
                "{sender} changed the channel name to {}",
                n.content.name
            ))]
        },
        AnySyncStateEvent::RoomEncryption(SyncStateEvent::Original(_)) => {
            vec![Line::Info(format!(
                "{sender} enabled end-to-end encryption"
            ))]
        },
        AnySyncStateEvent::RoomCreate(SyncStateEvent::Original(_)) => {
            vec![Line::Info(format!("{sender} created the channel"))]
        },
        AnySyncStateEvent::RoomPowerLevels(SyncStateEvent::Original(p)) => {
            let old = p
                .unsigned
                .prev_content
                .as_ref()
                .map(|c| c.users.clone())
                .unwrap_or_default();
            let new = &p.content.users;
            let mut modes = vec![];
            for user in old.keys().chain(new.keys()) {
                let was = old.get(user).is_some_and(|l| *l >= Int::from(50));
                let is = new.get(user).is_some_and(|l| *l >= Int::from(50));
                let m = match (was, is) {
                    (false, true) => "+o",
                    (true, false) => "-o",
                    _ => continue,
                };
                let entry = format!("{m} {}", nick_of(user));
                if !modes.contains(&entry) {
                    modes.push(entry);
                }
            }
            if modes.is_empty() {
                vec![]
            } else {
                vec![Line::Mode { modes: modes.join(" "), nick: sender }]
            }
        },
        _ => vec![],
    }
}

/// Translate one timeline event. Unknown/irrelevant events yield nothing.
pub fn convert(me: &UserId, tev: &TimelineEvent) -> Vec<Stamped> {
    if let TimelineEventKind::UnableToDecrypt { event, utd_info } = &tev.kind {
        let id = event.get_field::<String>("event_id").ok().flatten();
        let sender = event.get_field::<OwnedUserId>("sender").ok().flatten();
        let nick = match &sender {
            Some(s) => nick_of(s),
            None => "?".to_string(),
        };
        let ts = event.get_field::<i64>("origin_server_ts").ok().flatten();
        let reason = utd_reason(&utd_info.reason);
        return vec![(id, ts, Line::Undecryptable { nick, reason })];
    }
    convert_raw(me, tev.raw())
}

/// Translate one (decrypted or plain) timeline event.
pub fn convert_raw(me: &UserId, raw: &Raw<AnySyncTimelineEvent>) -> Vec<Stamped> {
    let Ok(ev) = raw.deserialize() else {
        return vec![];
    };
    let id = Some(ev.event_id().to_string());
    let ts = i64::try_from(u64::from(ev.origin_server_ts().0)).ok();
    let own = ev.sender() == me;
    let sender = ev.sender().to_string();
    let nick = nick_of(ev.sender());

    let lines = match ev {
        AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
            SyncMessageLikeEvent::Original(m),
        )) => {
            let line = match m.content.msgtype {
                MessageType::Text(t) => Line::Public { nick, own, text: t.body },
                MessageType::Emote(e) => Line::Action { nick, own, text: e.body },
                MessageType::Notice(n) => Line::Notice { nick, mxid: sender.clone(), text: n.body },
                MessageType::VerificationRequest(_) => {
                    Line::Info(verification_event_text(&nick, "request"))
                },
                other => {
                    let text = format!("[{}] {}", other.msgtype(), other.body());
                    Line::Public { nick, own, text }
                },
            };
            vec![line]
        },
        AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomEncrypted(_)) => {
            vec![Line::Undecryptable { nick, reason: "not decrypted yet".into() }]
        },
        AnySyncTimelineEvent::MessageLike(m)
            if m.event_type().to_string().starts_with("m.key.verification.") =>
        {
            let ty = m.event_type().to_string();
            let kind = ty.trim_start_matches("m.key.verification.");
            vec![Line::Info(verification_event_text(&nick, kind))]
        },
        AnySyncTimelineEvent::State(s) => state_lines(s),
        _ => vec![],
    };

    lines.into_iter().map(|l| (id.clone(), ts, l)).collect()
}

/// The last 50 events of a room, oldest first.
pub async fn backfill(room: &Room, me: &UserId) -> anyhow::Result<Vec<Stamped>> {
    let mut opts = MessagesOptions::backward();
    opts.limit = uint!(50);
    let msgs = room.messages(opts).await?;
    let mut out = vec![];
    for ev in msgs.chunk.iter().rev() {
        out.extend(convert(me, ev));
    }
    Ok(out)
}

/// Run the sync loop forever, feeding the UI. On `M_UNKNOWN_TOKEN` / soft
/// logout it runs [`login::recover`] (re-read session.json under the lock,
/// adopt it or refresh, retry once) and resumes; otherwise it prints the
/// relogin hint and stops.
pub fn spawn_sync(client: Client, tx: Tx, app_settings: ApplicationSettings) {
    // live client only (`--smoke` never gets here): refresh before expiry;
    // the 401 recovery below stays as the safety net
    crate::token_refresh::spawn(client.clone(), app_settings.clone(), Mode::Live);
    tokio::spawn(async move {
        let me = client.user_id().expect("logged in").to_owned();
        let attempts = Arc::new(AtomicU32::new(0));
        loop {
            let err = match sync_until_error(&client, &me, &tx, &attempts).await {
                Ok(()) => break,
                Err(e) if login::is_unknown_token(&e) => e,
                Err(e) => {
                    let line = Line::Error(format!("sync: {e}"));
                    let _ = tx.send(MxEvent::Info { room_id: None, line });
                    break;
                },
            };
            tracing::warn!("sync: {err}");
            let n = attempts.fetch_add(1, Ordering::SeqCst);
            match login::recover(&client, &app_settings, Mode::Live, n).await {
                Ok(None) => {},
                Ok(Some(msg)) => {
                    let _ = tx.send(MxEvent::Info { room_id: None, line: Line::Error(msg) });
                },
                Err(e) => {
                    let line = Line::Error(format!("{e}"));
                    let _ = tx.send(MxEvent::Info { room_id: None, line });
                    break;
                },
            }
        }
    });
}

/// `sync_with_callback` runs: returns when a sync request fails or the UI is
/// gone. Every sync response resets the recovery `attempts` counter. When the
/// away state changes, the run breaks after the current response and restarts
/// with a matching `set_presence` (the stored sync token is reused).
async fn sync_until_error(
    client: &Client,
    me: &OwnedUserId,
    tx: &Tx,
    attempts: &Arc<AtomicU32>,
) -> Result<(), matrix_sdk::Error> {
    let mut away_rx = SYNC_AWAY.subscribe();
    loop {
        let away = *away_rx.borrow_and_update();
        let settings = sync_settings(away);
        sync_run(client, me, tx, attempts, settings, &away_rx).await?;
        if tx.is_closed() {
            return Ok(());
        }
        tracing::debug!("sync: away changed, restarting the sync loop");
    }
}

/// One `sync_with_callback` run with fixed settings; breaks when the UI is
/// gone or the away state changed.
async fn sync_run(
    client: &Client,
    me: &OwnedUserId,
    tx: &Tx,
    attempts: &Arc<AtomicU32>,
    settings: SyncSettings,
    away_rx: &watch::Receiver<bool>,
) -> Result<(), matrix_sdk::Error> {
    client
        .sync_with_callback(settings, |resp| {
            let client = client.clone();
            let tx = tx.clone();
            let me = me.clone();
            attempts.store(0, Ordering::SeqCst);
            let away_changed = away_rx.has_changed().unwrap_or(false);
            async move {
                for (room_id, update) in resp.rooms.joined {
                    let Some(room) = client.get_room(&room_id) else {
                        continue;
                    };
                    let _ = tx.send(MxEvent::Room(snapshot(&room, &me).await));
                    let mut lines = vec![];
                    for ev in &update.timeline.events {
                        lines.extend(convert(&me, ev));
                    }
                    if !lines.is_empty() {
                        let _ = tx.send(MxEvent::Lines { room_id, lines, backfill: false });
                    }
                }
                for raw in &resp.presence {
                    if let Ok(ev) = raw.deserialize() {
                        let away =
                            away_of(ev.content.presence.as_str(), ev.content.status_msg.as_deref());
                        let _ = tx.send(MxEvent::Presence { user: ev.sender, away });
                    }
                }
                for room_id in resp.rooms.left.into_keys() {
                    let _ = tx.send(MxEvent::Left(room_id));
                }
                for room_id in resp.rooms.invited.into_keys() {
                    let line = Line::Info(format!(
                        "You have been invited to {room_id} -- /join {room_id}"
                    ));
                    let _ = tx.send(MxEvent::Info { room_id: None, line });
                }
                if tx.is_closed() || away_changed {
                    LoopCtrl::Break
                } else {
                    LoopCtrl::Continue
                }
            }
        })
        .await
}

/// Measure the round trip to the homeserver every minute (`[Lag n]`).
pub fn spawn_lag(client: Client, tx: Tx) {
    tokio::spawn(async move {
        loop {
            let start = Instant::now();
            if client.whoami().await.is_ok() &&
                tx.send(MxEvent::Lag(start.elapsed().as_secs())).is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

fn room_of(client: &Client, room_id: &OwnedRoomId) -> anyhow::Result<Room> {
    client
        .get_room(room_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown channel {room_id}"))
}

/// The DM with `user`, created if needed. New DMs are end-to-end encrypted:
/// `create_dm` puts `m.room.encryption` (megolm) in the initial state.
async fn dm_room(client: &Client, user: &UserId) -> anyhow::Result<Room> {
    match client.get_dm_room(user) {
        Some(room) => Ok(room),
        None => Ok(client.create_dm(user).await?),
    }
}

async fn send(room: &Room, me: &UserId, text: String, emote: bool, tx: &Tx) -> anyhow::Result<()> {
    let content = if emote {
        RoomMessageEventContent::emote_plain(text.clone())
    } else {
        RoomMessageEventContent::text_plain(text.clone())
    };
    let resp = room.send(content).await?;
    let nick = nick_of(me);
    let line = if emote {
        Line::Action { nick, own: true, text }
    } else {
        Line::Public { nick, own: true, text }
    };
    let _ = tx.send(MxEvent::Lines {
        room_id: room.room_id().to_owned(),
        lines: vec![(Some(resp.response.event_id.to_string()), None, line)],
        backfill: false,
    });
    Ok(())
}

/// The homeserver answered M_NOT_FOUND (unknown alias).
fn is_not_found(e: &matrix_sdk::Error) -> bool {
    e.client_api_error_kind() == Some(&ErrorKind::NotFound)
}

/// Fetch up to [`LIST_MAX`] public channels from the directory of `server`
/// (our own homeserver if `None`), optionally filtered by `search`.
pub async fn public_channels(
    client: &Client,
    server: Option<&str>,
    search: Option<&str>,
) -> anyhow::Result<Vec<ListEntry>> {
    let server = match server {
        Some(s) => {
            Some(OwnedServerName::try_from(s).map_err(|e| anyhow::anyhow!("bad server {s}: {e}"))?)
        },
        None => None,
    };
    let mut out = vec![];
    let mut since = None;
    loop {
        let mut req = PublicRoomsRequest::new();
        req.server = server.clone();
        req.limit = Some(uint!(50));
        req.since = since.take();
        req.filter.generic_search_term = search.map(ToOwned::to_owned);
        let resp = client.public_rooms_filtered(req).await?;
        for c in resp.chunk {
            out.push(ListEntry {
                alias: c.canonical_alias.map(|a| a.to_string()),
                room_id: c.room_id.to_string(),
                name: c.name.map(|n| sanitize(&n)),
                members: u64::from(c.num_joined_members),
                topic: c.topic.map(|t| sanitize(&t)),
                space: c.room_type == Some(RoomType::Space),
            });
        }
        match resp.next_batch {
            Some(next) if out.len() < LIST_MAX => since = Some(next),
            _ => break,
        }
    }
    out.truncate(LIST_MAX);
    Ok(out)
}

async fn list(client: &Client, me: &UserId, args: ListArgs, tx: &Tx) -> anyhow::Result<()> {
    let entries = public_channels(client, args.server.as_deref(), args.search.as_deref()).await?;
    let server = args.server.unwrap_or_else(|| me.server_name().to_string());
    let _ = tx.send(MxEvent::List { server, search: args.search, entries });
    Ok(())
}

async fn run(client: &Client, effect: Effect, tx: &Tx) -> anyhow::Result<()> {
    let me = client
        .user_id()
        .ok_or_else(|| anyhow::anyhow!("not logged in"))?
        .to_owned();

    match effect {
        Effect::Send { room_id, text, emote } => {
            send(&room_of(client, &room_id)?, &me, text, emote, tx).await?;
        },
        Effect::Join(target) => {
            let id = OwnedRoomOrAliasId::try_from(target.id.as_str())
                .map_err(|e| anyhow::anyhow!("Bad channel name {}: {e}", target.id))?;
            let via: Vec<OwnedServerName> = target
                .via
                .iter()
                .filter_map(|s| OwnedServerName::try_from(s.as_str()).ok())
                .collect();
            let t0 = std::time::Instant::now();
            let room = match client.join_room_by_id_or_alias(&id, &via).await {
                Ok(room) => room,
                Err(e) if is_not_found(&e) => {
                    let msg = if target.implicit_server {
                        not_found_hint(&target.id)
                    } else {
                        format!("Cannot join channel {} (no such channel)", target.id)
                    };
                    let _ = tx.send(MxEvent::Info { room_id: None, line: Line::Error(msg) });
                    return Ok(());
                },
                Err(e) => return Err(e.into()),
            };
            let _ = tx.send(MxEvent::Room(snapshot(&room, &me).await));
            let millis = t0.elapsed().as_millis() as u64;
            let _ = tx.send(MxEvent::Synched { room_id: room.room_id().to_owned(), millis });
        },
        Effect::List(args) => list(client, &me, args, tx).await?,
        Effect::Part { room_id, .. } => {
            room_of(client, &room_id)?.leave().await?;
            let _ = tx.send(MxEvent::Left(room_id));
        },
        Effect::Topic { room_id, topic } => {
            room_of(client, &room_id)?.set_room_topic(&topic).await?;
        },
        Effect::Nick(nick) => {
            client.account().set_display_name(Some(&nick)).await?;
            let _ = tx.send(MxEvent::OwnDisplayName(nick));
        },
        Effect::Msg { user, text } => {
            let room = dm_room(client, &user).await?;
            let _ = tx.send(MxEvent::Room(snapshot(&room, &me).await));
            send(&room, &me, text, false, tx).await?;
        },
        Effect::Query(user) => {
            let room = dm_room(client, &user).await?;
            let _ = tx.send(MxEvent::Room(snapshot(&room, &me).await));
            let _ = tx.send(MxEvent::Focus(room.room_id().to_owned()));
        },
        Effect::Invite { room_id, user } => {
            room_of(client, &room_id)?.invite_user_by_id(&user).await?;
            let line = Line::Info(format!("Inviting {user} to {room_id}"));
            let _ = tx.send(MxEvent::Info { room_id: Some(room_id), line });
        },
        Effect::Kick { room_id, user, reason } => {
            room_of(client, &room_id)?.kick_user(&user, reason.as_deref()).await?;
        },
        Effect::Power { room_id, user, level } => {
            room_of(client, &room_id)?
                .update_power_levels(vec![(&user, Int::from(level as i32))])
                .await?;
        },
        Effect::Away(reason) => {
            let (presence, status_msg) = away_presence(reason.as_deref());
            // the sync loop first, so the next /sync already carries it
            SYNC_AWAY.send_replace(reason.is_some());
            let mut req = PresenceRequest::new(me.clone(), presence);
            req.status_msg = status_msg;
            client.send(req).await?;
            let _ = tx.send(MxEvent::Away(reason));
        },
        Effect::Whois(user) => {
            // The display name is the BitchX ircname (real name); the nick is
            // the localpart.
            let name = client
                .account()
                .fetch_profile_field_of_static::<DisplayName>(user.clone())
                .await
                .ok()
                .flatten()
                .map(|n| sanitize(&n))
                .unwrap_or_else(|| user.localpart().to_string());
            let mut chans = vec![];
            for room in client.joined_rooms() {
                if let Ok(Some(m)) = room.get_member_no_sync(&user).await {
                    let flag = if power(m.power_level()) >= 50 {
                        "@"
                    } else {
                        ""
                    };
                    chans.push(format!("{flag}{}", channel_name(&room, &me, None)));
                }
            }
            // WHOIS_AWAY `| away     : $0 - $1-` (include/color.h:726).
            let away = client
                .send(GetPresenceRequest::new(user.clone()))
                .await
                .ok()
                .and_then(|p| away_of(p.presence.as_str(), p.status_msg.as_deref()));
            let _ = tx.send(MxEvent::Presence { user: user.clone(), away: away.clone() });
            let devices = client
                .encryption()
                .get_user_devices(&user)
                .await
                .map(|d| d.devices().count())
                .unwrap_or(0);
            let ident = match client.encryption().get_user_identity(&user).await {
                Ok(Some(i)) if i.is_verified() => "verified",
                Ok(Some(_)) => "unverified",
                _ => "unknown",
            };
            let send = |line: Line| tx.send(MxEvent::Info { room_id: None, line });
            let _ = send(Line::WhoisRule);
            let _ = send(Line::WhoisNick {
                nick: nick_of(&user),
                mxid: user.to_string(),
                info: ident.to_string(),
            });
            let mut rows = vec![
                ("ircname", name),
                ("server", user.server_name().to_string()),
            ];
            if !chans.is_empty() {
                rows.insert(1, ("channels", chans.join(" ")));
            }
            if let Some(msg) = away {
                rows.push(("away", format!("{} - {msg}", nick_of(&user))));
            }
            rows.push(("devices", devices.to_string()));
            let len = rows.len();
            for (i, (key, value)) in rows.into_iter().enumerate() {
                let _ = send(Line::WhoisRow {
                    row: (i + 1) as u8,
                    last: i + 1 == len,
                    key: key.to_string(),
                    value,
                });
            }
        },
        Effect::Backfill(room_id) => {
            let room = room_of(client, &room_id)?;
            let lines = backfill(&room, &me).await?;
            let _ = tx.send(MxEvent::Lines { room_id, lines, backfill: true });
        },
        Effect::EnableEncryption(room_id) => {
            let room = room_of(client, &room_id)?;
            crate::e2ee::enable_encryption(&room, tx).await?;
            let _ = tx.send(MxEvent::Room(snapshot(&room, &me).await));
        },
        Effect::Verify { user, flow_id, action } => {
            crate::e2ee::verify(client, user, flow_id, action, tx).await?;
        },
        Effect::Devices(user) => crate::e2ee::devices(client, user, tx).await?,
        Effect::Trust => crate::e2ee::trust(client, tx).await?,
        Effect::Recover(key) => crate::e2ee::recover(client, key, tx).await?,
        Effect::Quit => {},
    }
    Ok(())
}

/// Execute an effect in the background; failures become `-:-` error lines.
pub fn spawn_effect(client: Client, effect: Effect, tx: Tx) {
    tokio::spawn(async move {
        let what = format!("{effect:?}");
        let what = what.split([' ', '(', '{']).next().unwrap_or("").to_lowercase();
        if let Err(e) = run(&client, effect, &tx).await {
            let line = Line::Error(format!("{what}: {e}"));
            let _ = tx.send(MxEvent::Info { room_id: None, line });
        }
    });
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::{owned_room_id, owned_user_id};

    use super::*;
    use crate::bitchx::app::App;
    use crate::bitchx::app::tests::{app, typed};

    /// What the sync loop's `set_presence` becomes after these effects ran
    /// (the [`Effect::Away`] executor drives [`SYNC_AWAY`]); `None` if no
    /// away change.
    fn sync_after(fx: &[Effect]) -> Option<PresenceState> {
        fx.iter().rev().find_map(|e| {
            match e {
                Effect::Away(reason) => Some(sync_presence(reason.is_some())),
                _ => None,
            }
        })
    }

    #[test]
    fn test_sync_presence_decision() {
        assert_eq!(sync_presence(true), PresenceState::Unavailable);
        assert_eq!(sync_presence(false), PresenceState::Online);
        // The settings really carry it (default would be `online`).
        assert!(format!("{:?}", sync_settings(true)).to_lowercase().contains("unavailable"));
        assert!(format!("{:?}", sync_settings(false)).to_lowercase().contains("online"));
        // Away: unavailable + reason; back: online + status_msg cleared.
        assert_eq!(
            away_presence(Some("lunch")),
            (PresenceState::Unavailable, Some("lunch".to_string()))
        );
        assert_eq!(away_presence(None), (PresenceState::Online, None));
    }

    #[test]
    fn test_sync_presence_follows_manual_away() {
        let mut app = app();
        let fx = typed(&mut app, "/away at lunch");
        assert!(!fx.iter().any(|e| matches!(e, Effect::Nick(_))), "away never changes the nick");
        assert_eq!(sync_after(&fx), Some(PresenceState::Unavailable));
        app.handle_mx(MxEvent::Away(Some("at lunch".into())));
        // A keypress does not end a manual away: still unavailable.
        assert_eq!(sync_after(&app.activity()), None);
        let fx = typed(&mut app, "/back");
        assert_eq!(sync_after(&fx), Some(PresenceState::Online));
        assert_eq!(fx, vec![Effect::Away(None)], "back clears status_msg, no nick change");
    }

    #[test]
    fn test_sync_presence_follows_auto_away() {
        let t0 = 1_764_000_000_000;
        let mut app = app();
        app.fake_now = Some(t0);
        app.last_input = t0;
        app.auto_away_armed = true;
        app.fake_now = Some(t0 + 600_000);
        let fx = app.tick();
        assert_eq!(sync_after(&fx), Some(PresenceState::Unavailable));
        assert!(!fx.iter().any(|e| matches!(e, Effect::Nick(_))), "away never changes the nick");
        // First keypress: auto-back -> online, status_msg cleared.
        let fx = app.activity();
        assert_eq!(fx, vec![Effect::Away(None)]);
        assert_eq!(sync_after(&fx), Some(PresenceState::Online));
        assert_eq!(away_presence(None).1, None);
    }

    fn ev(json: &str) -> Raw<AnySyncTimelineEvent> {
        Raw::from_json_string(json.to_string()).unwrap()
    }

    fn member_ev(id: u8, sender: &str, target: &str, content: &str, prev: Option<&str>) -> String {
        let unsigned = prev.map(|p| format!(r#","unsigned":{{"prev_content":{p}}}"#));
        format!(
            r#"{{"type":"m.room.member","event_id":"$m{id}","sender":"{sender}","state_key":"{target}","origin_server_ts":{id},"content":{content}{}}}"#,
            unsigned.unwrap_or_default()
        )
    }

    #[test]
    fn test_nick_is_localpart_not_display_name() {
        let me = owned_user_id!("@carla:x");
        let m = Member::new(me.clone(), 100);
        assert_eq!(m.nick, "carla");
        assert_eq!(nick_of(&owned_user_id!("@Armin:remote.tld")), "Armin");

        let mut app = App::new(me.clone());
        app.clock = Some("09:24pm".into());
        let room_id = owned_room_id!("!r:x");
        app.handle_mx(MxEvent::Room(RoomSnapshot {
            room_id: Some(room_id.clone()),
            name: "#room".into(),
            members: vec![m, Member::new(owned_user_id!("@armin:remote.tld"), 0)],
            ..Default::default()
        }));
        app.switch(1);

        let join = r#"{"membership":"join","displayname":"Carla"}"#;
        let events = [
            member_ev(1, "@carla:x", "@carla:x", join, None),
            r#"{"type":"m.room.message","event_id":"$t","sender":"@carla:x","origin_server_ts":2,"content":{"msgtype":"m.text","body":"hello"}}"#.to_string(),
            r#"{"type":"m.room.message","event_id":"$u","sender":"@armin:remote.tld","origin_server_ts":3,"content":{"msgtype":"m.text","body":"sup"}}"#.to_string(),
            member_ev(4, "@carla:x", "@carla:x", r#"{"membership":"join","displayname":"CARLA"}"#, Some(join)),
            member_ev(
                5,
                "@carla:x",
                "@armin:remote.tld",
                r#"{"membership":"leave","displayname":"Armin"}"#,
                Some(r#"{"membership":"join","displayname":"Armin"}"#),
            ),
        ];
        let lines: Vec<_> = events.iter().flat_map(|e| convert_raw(&me, &ev(e))).collect();
        // Each line carries its event's origin_server_ts (millis).
        assert_eq!(lines[1].1, Some(2));
        assert_eq!(lines[2].1, Some(3));
        assert_eq!(lines[1].2, Line::Public {
            nick: "carla".into(),
            own: true,
            text: "hello".into()
        });
        app.handle_mx(MxEvent::Lines { room_id, lines, backfill: false });

        let rows = crate::smoke::render_text(&mut app, 60, 12).unwrap();
        assert!(rows[0].starts_with(" carla [@carla:x] has joined #room"), "{rows:?}");
        assert!(rows[1].starts_with("<carla> hello"), "{rows:?}");
        assert!(rows[2].starts_with("<armin> sup"), "{rows:?}");
        assert!(rows[3].starts_with(" carla changed display name to CARLA"), "{rows:?}");
        assert!(rows[4].starts_with(" armin was kicked off #room by carla"), "{rows:?}");
        assert!(rows[9].starts_with(" [09:24pm][@carla] [#room]"), "{:?}", rows[9]);
        assert!(!rows.iter().any(|r| r.contains("<Carla>") || r.contains("Armin")), "{rows:?}");

        // /nick only changes the display name; the nick stays the localpart.
        app.handle_mx(MxEvent::OwnDisplayName("Carla".into()));
        assert_eq!(app.nick, "carla");
        assert_eq!(app.status().nick, "carla");
        assert_eq!(
            app.windows[0].lines.last(),
            Some(&Line::DisplayName { nick: "carla".into(), name: Some("Carla".into()) })
        );
    }
}
