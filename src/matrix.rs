//! # The Matrix side
//!
//! A sync loop on matrix-sdk that turns timeline events into BitchX
//! [`Line`]s, room snapshots for the windows, history backfill, and the
//! executor for the [`Effect`]s the UI asks for. Everything it learns goes
//! to the UI as [`MxEvent`]s over a channel.
use std::time::{Duration, Instant};

use matrix_sdk::config::SyncSettings;
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::api::client::presence::set_presence::v3::Request as PresenceRequest;
use matrix_sdk::ruma::api::client::profile::DisplayName;
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
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{Int, OwnedRoomId, OwnedRoomOrAliasId, UserId, uint};
use matrix_sdk::{Client, LoopCtrl, Room, RoomMemberships};
use tokio::sync::mpsc::UnboundedSender;

use crate::bitchx::format::sanitize;
use crate::bitchx::model::{Effect, Line, Member, MxEvent, RoomSnapshot};

pub type Tx = UnboundedSender<MxEvent>;

/// The member's nick: their room display name, else their localpart.
pub async fn nick_of(room: &Room, user: &UserId) -> String {
    match room.get_member_no_sync(user).await {
        Ok(Some(m)) => {
            m.display_name()
                .filter(|n| !n.trim().is_empty())
                .map(sanitize)
                .unwrap_or_else(|| user.localpart().to_string())
        },
        _ => user.localpart().to_string(),
    }
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
        .map(|m| {
            Member {
                nick: m
                    .display_name()
                    .filter(|n| !n.trim().is_empty())
                    .map(sanitize)
                    .unwrap_or_else(|| m.user_id().localpart().to_string()),
                user_id: m.user_id().to_owned(),
                power: power(m.power_level()),
            }
        })
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
    let peer_nick = match &dm_peer {
        Some(p) => Some(nick_of(room, p).await),
        None => None,
    };

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

async fn state_lines(room: &Room, ev: AnySyncStateEvent) -> Vec<Line> {
    let sender = nick_of(room, ev.sender()).await;

    match ev {
        AnySyncStateEvent::RoomMember(SyncStateEvent::Original(m)) => {
            let Ok(target) = UserId::parse(m.state_key.as_str()) else {
                return vec![];
            };
            let mxid = target.to_string();
            let local = target.localpart().to_string();
            let named = |n: Option<&str>| {
                n.filter(|n| !n.trim().is_empty())
                    .map(sanitize)
                    .unwrap_or_else(|| local.clone())
            };
            let prev_name = m.prev_content().and_then(|p| p.displayname.clone());
            let victim = named(m.content.displayname.as_deref().or(prev_name.as_deref()));
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
                    vec![Line::Nick { old: named(c.old), new: named(c.new) }]
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
                "{sender} changed the room name to {}",
                n.content.name
            ))]
        },
        AnySyncStateEvent::RoomEncryption(SyncStateEvent::Original(_)) => {
            vec![Line::Info(format!(
                "{sender} enabled end-to-end encryption"
            ))]
        },
        AnySyncStateEvent::RoomCreate(SyncStateEvent::Original(_)) => {
            vec![Line::Info(format!("{sender} created the room"))]
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
                let entry = format!("{m} {}", nick_of(room, user).await);
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
pub async fn convert(
    room: &Room,
    me: &UserId,
    raw: &Raw<AnySyncTimelineEvent>,
) -> Vec<(Option<String>, Line)> {
    let Ok(ev) = raw.deserialize() else {
        return vec![];
    };
    let id = Some(ev.event_id().to_string());
    let own = ev.sender() == me;
    let nick = nick_of(room, ev.sender()).await;

    let lines = match ev {
        AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
            SyncMessageLikeEvent::Original(m),
        )) => {
            let line = match m.content.msgtype {
                MessageType::Text(t) => Line::Public { nick, own, text: t.body },
                MessageType::Emote(e) => Line::Action { nick, own, text: e.body },
                MessageType::Notice(n) => Line::Notice { nick, text: n.body },
                other => {
                    let text = format!("[{}] {}", other.msgtype(), other.body());
                    Line::Public { nick, own, text }
                },
            };
            vec![line]
        },
        AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomEncrypted(_)) => {
            vec![Line::Public {
                nick,
                own,
                text: "** unable to decrypt message **".into(),
            }]
        },
        AnySyncTimelineEvent::State(s) => state_lines(room, s).await,
        _ => vec![],
    };

    lines.into_iter().map(|l| (id.clone(), l)).collect()
}

/// The last 50 events of a room, oldest first.
pub async fn backfill(room: &Room, me: &UserId) -> anyhow::Result<Vec<(Option<String>, Line)>> {
    let mut opts = MessagesOptions::backward();
    opts.limit = uint!(50);
    let msgs = room.messages(opts).await?;
    let mut out = vec![];
    for ev in msgs.chunk.iter().rev() {
        out.extend(convert(room, me, ev.raw()).await);
    }
    Ok(out)
}

/// Run the sync loop forever, feeding the UI.
pub fn spawn_sync(client: Client, tx: Tx) {
    tokio::spawn(async move {
        let me = client.user_id().expect("logged in").to_owned();
        let settings = SyncSettings::default().timeout(Duration::from_secs(30));
        let res = client
            .sync_with_callback(settings, |resp| {
                let client = client.clone();
                let tx = tx.clone();
                let me = me.clone();
                async move {
                    for (room_id, update) in resp.rooms.joined {
                        let Some(room) = client.get_room(&room_id) else {
                            continue;
                        };
                        let _ = tx.send(MxEvent::Room(snapshot(&room, &me).await));
                        let mut lines = vec![];
                        for ev in &update.timeline.events {
                            lines.extend(convert(&room, &me, ev.raw()).await);
                        }
                        if !lines.is_empty() {
                            let _ = tx.send(MxEvent::Lines { room_id, lines, backfill: false });
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
                    if tx.is_closed() {
                        LoopCtrl::Break
                    } else {
                        LoopCtrl::Continue
                    }
                }
            })
            .await;

        if let Err(e) = res {
            let _ = tx.send(MxEvent::Info {
                room_id: None,
                line: Line::Error(format!("sync: {e}")),
            });
        }
    });
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
        .ok_or_else(|| anyhow::anyhow!("Unknown room {room_id}"))
}

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
    let nick = nick_of(room, me).await;
    let line = if emote {
        Line::Action { nick, own: true, text }
    } else {
        Line::Public { nick, own: true, text }
    };
    let _ = tx.send(MxEvent::Lines {
        room_id: room.room_id().to_owned(),
        lines: vec![(Some(resp.response.event_id.to_string()), line)],
        backfill: false,
    });
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
            let target = if target.starts_with('#') && !target.contains(':') {
                format!("{target}:{}", me.server_name())
            } else {
                target
            };
            let id = OwnedRoomOrAliasId::try_from(target.as_str())
                .map_err(|e| anyhow::anyhow!("{target}: {e}"))?;
            let room = client.join_room_by_id_or_alias(&id, &[]).await?;
            let _ = tx.send(MxEvent::Room(snapshot(&room, &me).await));
            let _ = tx.send(MxEvent::Focus(room.room_id().to_owned()));
        },
        Effect::Part { room_id, .. } => {
            room_of(client, &room_id)?.leave().await?;
            let _ = tx.send(MxEvent::Left(room_id));
        },
        Effect::Topic { room_id, topic } => {
            room_of(client, &room_id)?.set_room_topic(&topic).await?;
        },
        Effect::Nick(nick) => {
            client.account().set_display_name(Some(&nick)).await?;
            let _ = tx.send(MxEvent::OwnNick(nick));
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
            let presence = if reason.is_some() {
                PresenceState::Unavailable
            } else {
                PresenceState::Online
            };
            let mut req = PresenceRequest::new(me.clone(), presence);
            req.status_msg = reason.clone();
            client.send(req).await?;
            let _ = tx.send(MxEvent::Away(reason));
        },
        Effect::Whois(user) => {
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
            let info = |s: String| MxEvent::Info { room_id: None, line: Line::Info(s) };
            let _ = tx.send(info(format!("{name} [{user}]")));
            let _ = tx.send(info(format!(" server   : {}", user.server_name())));
            if !chans.is_empty() {
                let _ = tx.send(info(format!(" channels : {}", chans.join(" "))));
            }
            let _ = tx.send(info(format!("End of /WHOIS for {user}")));
        },
        Effect::Backfill(room_id) => {
            let room = room_of(client, &room_id)?;
            let lines = backfill(&room, &me).await?;
            let _ = tx.send(MxEvent::Lines { room_id, lines, backfill: true });
        },
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
