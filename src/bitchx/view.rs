//! # BitchX formatting
//!
//! Timeline events as BitchX lines, the numbered windows, and the status
//! window. Pure data in, `Line`s out; the layout lives in `screen.rs`.
//!
//! Formats follow the "default bx" section of BitchX1.3 `include/color.h`
//! (STATUS_FORMAT1/2, FORMAT_*_FSET) and `source/status.c`; see
//! `docs/bitchx-formats.md`.
use matrix_sdk::ruma::events::room::member::MembershipChange;
use matrix_sdk::ruma::events::room::power_levels::UserPowerLevel;
use matrix_sdk::ruma::events::{AnyStateEventContentChange, StateEventContentChange};
use matrix_sdk::ruma::int;

use crate::bitchx::status::{BxStatus, UserCounts};
use crate::prelude::*;
use crate::windows::room::ChatState;

pub fn fg(c: Color) -> Style {
    Style::default().fg(c)
}

pub fn bold(c: Color) -> Style {
    fg(c).add_modifier(StyleModifier::BOLD)
}

/// `%n`: back to the terminal's default colours.
pub fn n() -> Style {
    Style::default()
}

/// `%K`: dark gray (bold black).
pub fn k() -> Style {
    bold(Color::DarkGray)
}

/// `%W`: bold bright white.
pub fn w() -> Style {
    bold(Color::White)
}

pub fn sp(text: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(sanitize(&text.into()), style)
}

/// Replace control characters (escape sequences, newlines) with spaces.
pub fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// `$G` (SHOW_NUMERICS_STR, ASCII build): bold red `-`, bold white `:`, bold red `-`.
pub fn g() -> Vec<Span<'static>> {
    vec![
        Span::styled("-", bold(Color::LightRed)),
        Span::styled(":", w()),
        Span::styled("-", bold(Color::LightRed)),
        Span::raw(" "),
    ]
}

/// A `$G text` line.
pub fn notice_line(text: &str, style: Style) -> Line<'static> {
    let mut spans = g();
    spans.push(sp(text, style));
    Line::from(spans)
}

fn g_line(rest: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = g();
    spans.extend(rest);
    Line::from(spans)
}

/// What a message needs to know about its room.
pub struct FmtCtx<'a> {
    pub me: &'a UserId,
    pub info: &'a RoomInfo,
    pub channel: &'a str,
}

impl FmtCtx<'_> {
    /// The member's nick: their room display name, else their localpart.
    pub fn nick(&self, user: &UserId) -> String {
        match self.info.display_names.get(user) {
            Some(name) if !name.trim().is_empty() => sanitize(&name),
            _ => user.localpart().to_string(),
        }
    }
}

/// Format one timeline event as BitchX lines.
pub fn format_message(msg: &Message, ctx: &FmtCtx<'_>) -> Vec<Line<'static>> {
    if let MessageEvent::State(ev) = &msg.event {
        return state_lines(ev, ctx);
    }

    let nick = ctx.nick(&msg.sender);
    let own = msg.sender == ctx.me;
    let body = msg.event.body();

    let prefix = match msg.event.msgtype() {
        // ACTION / SEND_ACTION: `%K* %W$nick %n$text`
        Some(MessageType::Emote(_)) => vec![sp("* ", k()), sp(nick, w()), sp(" ", n())],
        // PUBLIC_NOTICE: `%K-%P$nick%K:%p$chan%K-%n $text`
        Some(MessageType::Notice(_)) => {
            vec![
                sp("-", k()),
                sp(nick, fg(Color::LightMagenta)),
                sp(":", k()),
                sp(ctx.channel, fg(Color::Magenta)),
                sp("-", k()),
                sp(" ", n()),
            ]
        },
        // SEND_PUBLIC: `%P<%n$mynick%P>%n $text`
        _ if own => {
            vec![
                sp("<", fg(Color::LightMagenta)),
                sp(nick, n()),
                sp(">", fg(Color::LightMagenta)),
                sp(" ", n()),
            ]
        },
        // PUBLIC: `%B<%n$nick%B>%n $text`
        _ => {
            vec![
                sp("<", fg(Color::LightBlue)),
                sp(nick, n()),
                sp(">", fg(Color::LightBlue)),
                sp(" ", n()),
            ]
        },
    };

    let style = match msg.event {
        MessageEvent::Redacted(..) | MessageEvent::EncryptedRedacted(..) => k(),
        MessageEvent::EncryptedOriginal(..) => fg(Color::DarkGray),
        _ => n(),
    };

    let indent = " ".repeat(Line::from(prefix.clone()).width());
    let mut lines = vec![];

    for (i, text) in body.split('\n').enumerate() {
        let mut spans = if i == 0 {
            prefix.clone()
        } else {
            vec![Span::raw(indent.clone())]
        };
        spans.push(sp(text, style));
        lines.push(Line::from(spans));
    }

    lines
}

fn state_lines(ev: &AnySyncStateEvent, ctx: &FmtCtx<'_>) -> Vec<Line<'static>> {
    if let AnyStateEventContentChange::RoomMember(StateEventContentChange::Original {
        content,
        prev_content,
    }) = ev.content_change() &&
        let Ok(target) = UserId::parse(ev.state_key())
    {
        let prev = prev_content.as_ref().map(|p| p.details());
        if let MembershipChange::KickedAndBanned =
            content.membership_change(prev, ev.sender(), &target)
        {
            // IRC kickban: MODE +b, then KICK.
            let chan = ctx.channel.to_string();
            let victim = ctx.nick(&target);
            let sender = ctx.nick(ev.sender());
            let reason = content.reason.clone().filter(|r| !r.trim().is_empty());
            let mode = g_line(vec![
                sp("mode", n()),
                sp("/", k()),
                sp(chan.clone(), fg(Color::Cyan)),
                sp(" [", k()),
                sp(format!("+b {victim}"), w()),
                sp("]", k()),
                sp(" by ", n()),
                sp(sender.clone(), w()),
            ]);
            let mut s = vec![
                sp(format!("{victim} was kicked off {chan} by "), n()),
                sp(sender, fg(Color::Cyan)),
            ];
            if let Some(r) = reason {
                s.extend([sp(" (", k()), sp(r, n()), sp(")", k())]);
            }
            return vec![mode, g_line(s)];
        }
    }
    vec![state_line(ev, ctx)]
}

fn state_line(ev: &AnySyncStateEvent, ctx: &FmtCtx<'_>) -> Line<'static> {
    let chan = ctx.channel.to_string();
    let sender = ctx.nick(ev.sender());

    match ev.content_change() {
        AnyStateEventContentChange::RoomMember(StateEventContentChange::Original {
            content,
            prev_content,
        }) => {
            let Ok(target) = UserId::parse(ev.state_key()) else {
                return generic_state(ev, ctx);
            };
            let prev = prev_content.as_ref().map(|p| p.details());
            let change = content.membership_change(prev, ev.sender(), &target);
            let victim = ctx.nick(&target);
            let reason = content.reason.clone().filter(|r| !r.trim().is_empty());

            match change {
                // JOIN: `$G %C$nick %K[%c$userhost%K]%n has joined $chan`
                MembershipChange::Joined | MembershipChange::InvitationAccepted => {
                    g_line(vec![
                        sp(victim, fg(Color::LightCyan)),
                        sp(" [", k()),
                        sp(target.as_str(), fg(Color::Cyan)),
                        sp("]", k()),
                        sp(format!(" has joined {chan}"), n()),
                    ])
                },
                // LEAVE: `$G $nick %K[%w$userhost%K]%n has left $chan %K[%W$reason%K]`
                MembershipChange::Left |
                MembershipChange::InvitationRejected |
                MembershipChange::KnockRetracted => {
                    let mut s = vec![
                        sp(victim, n()),
                        sp(" [", k()),
                        sp(target.as_str(), fg(Color::Gray)),
                        sp("]", k()),
                        sp(format!(" has left {chan}"), n()),
                    ];
                    if let Some(r) = reason {
                        s.extend([sp(" [", k()), sp(r, w()), sp("]", k())]);
                    }
                    g_line(s)
                },
                // KICK: `$G %n$victim was kicked off $chan by %c$kicker %K(%n$reason%K)`
                MembershipChange::Kicked => {
                    let mut s = vec![
                        sp(format!("{victim} was kicked off {chan} by "), n()),
                        sp(sender, fg(Color::Cyan)),
                    ];
                    if let Some(r) = reason {
                        s.extend([sp(" (", k()), sp(r, n()), sp(")", k())]);
                    }
                    g_line(s)
                },
                // MODE: `$G %nmode%K/%c$chan %K[%W$modes%K]%n by %W$nick`
                MembershipChange::Banned | MembershipChange::Unbanned => {
                    let flag = if matches!(change, MembershipChange::Banned) {
                        "+b"
                    } else {
                        "-b"
                    };
                    g_line(vec![
                        sp("mode", n()),
                        sp("/", k()),
                        sp(chan, fg(Color::Cyan)),
                        sp(" [", k()),
                        sp(format!("{flag} {victim}"), w()),
                        sp("]", k()),
                        sp(" by ", n()),
                        sp(sender, w()),
                    ])
                },
                // NICKNAME: `$G %W$old %nis now known as %c$new`
                MembershipChange::ProfileChanged { displayname_change: Some(c), .. } => {
                    let local = target.localpart().to_string();
                    let old = c.old.map(str::to_string).unwrap_or_else(|| local.clone());
                    let new = c.new.map(str::to_string).unwrap_or(local);
                    g_line(vec![
                        sp(old, w()),
                        sp(" is now known as ", n()),
                        sp(new, fg(Color::Cyan)),
                    ])
                },
                MembershipChange::Invited => {
                    g_line(vec![
                        sp(sender, fg(Color::Cyan)),
                        sp(format!(" invited {victim} to {chan}"), n()),
                    ])
                },
                _ => generic_state(ev, ctx),
            }
        },
        // TOPIC_CHANGE: `$G Topic %K(%c$chan%K):%n changed by %c$nick%K:%n $topic`
        AnyStateEventContentChange::RoomTopic(StateEventContentChange::Original {
            content,
            ..
        }) => {
            g_line(vec![
                sp("Topic ", n()),
                sp("(", k()),
                sp(chan, fg(Color::Cyan)),
                sp("):", k()),
                sp(" changed by ", n()),
                sp(sender, fg(Color::Cyan)),
                sp(":", k()),
                sp(format!(" {}", content.topic), n()),
            ])
        },
        _ => generic_state(ev, ctx),
    }
}

/// Any other state event: `$G nick <description>`.
fn generic_state(ev: &AnySyncStateEvent, ctx: &FmtCtx<'_>) -> Line<'static> {
    let body = MessageEvent::State(Box::new(ev.clone())).body().into_owned();
    let text = body.strip_prefix("* ").unwrap_or(&body).to_string();

    g_line(vec![
        sp(ctx.nick(ev.sender()), fg(Color::Cyan)),
        sp(format!(" {text}"), n()),
    ])
}

/// Split lines so that none is wider than `width` cells.
pub fn wrap(lines: &[Line<'static>], width: u16) -> Vec<Line<'static>> {
    let width = width as usize;
    let mut out = vec![];

    if width == 0 {
        return out;
    }

    for line in lines {
        let mut cur: Vec<Span<'static>> = vec![];
        let mut used = 0;

        for span in &line.spans {
            let mut run = String::new();

            for g in span.content.graphemes(true) {
                let gw = UnicodeWidthStr::width(g);

                if used + gw > width && used > 0 {
                    if !run.is_empty() {
                        cur.push(Span::styled(std::mem::take(&mut run), span.style));
                    }
                    out.push(Line::from(std::mem::take(&mut cur)));
                    used = 0;
                }

                if gw > width {
                    // Can never fit; drop it rather than overflow.
                    continue;
                }

                run.push_str(g);
                used += gw;
            }

            if !run.is_empty() {
                cur.push(Span::styled(run, span.style));
            }
        }

        out.push(Line::from(cur));
    }

    out
}

/// One numbered BitchX window (a joined room).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BxWindow {
    pub refnum: usize,
    pub room_id: OwnedRoomId,
    pub title: String,
    pub unread: bool,
}

/// The refnum of the status window; rooms are numbered after it.
pub const STATUS_REFNUM: usize = 1;

/// Number joined rooms and DMs as windows 2, 3, ... (window 1 is the status
/// window), ordered by name so that the numbers stay put between redraws.
pub fn windows(store: &ProgramStore) -> Vec<BxWindow> {
    let app = &store.application;
    let mut rooms: Vec<(String, OwnedRoomId, bool)> = app
        .sync_info
        .rooms
        .iter()
        .chain(app.sync_info.dms.iter())
        .map(|r| {
            let id = r.room_id().to_owned();
            let unread = r.num_unread_messages() > 0 || r.is_marked_unread();
            (channel_name(r, store), id, unread)
        })
        .collect();

    rooms.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then(a.1.cmp(&b.1)));
    rooms.dedup_by(|a, b| a.1 == b.1);

    rooms
        .into_iter()
        .enumerate()
        .map(|(i, (title, room_id, unread))| {
            BxWindow {
                refnum: i + STATUS_REFNUM + 1,
                room_id,
                title,
                unread,
            }
        })
        .collect()
}

/// The `#channel` name for a room: its canonical alias, else its title.
pub fn channel_name(room: &MatrixRoom, store: &ProgramStore) -> String {
    room.canonical_alias()
        .map(|a| a.to_string())
        .unwrap_or_else(|| store.application.get_room_title(room.room_id()))
}

fn clock() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

fn own_nick(store: &ProgramStore, info: Option<&RoomInfo>) -> String {
    let me = &store.application.settings.user_id;

    info.and_then(|i| i.display_names.get(me))
        .filter(|n| !n.trim().is_empty())
        .map(|n| sanitize(&n))
        .unwrap_or_else(|| me.localpart().to_string())
}

fn act_list(wins: &[BxWindow], current: usize) -> Vec<usize> {
    wins.iter()
        .filter(|w| w.unread && w.refnum != current)
        .map(|w| w.refnum)
        .collect()
}

/// Whether a power level counts as channel operator (`@`).
pub fn is_op(pl: UserPowerLevel) -> bool {
    pl >= UserPowerLevel::Int(int!(50))
}

/// Whether a power level counts as voice (`+`).
pub fn is_voice(pl: UserPowerLevel) -> bool {
    !is_op(pl) && pl > UserPowerLevel::Int(int!(0))
}

/// Channel modes from Matrix room state: `i` for rooms you cannot just
/// join (invite, knock, restricted), `e` for end-to-end encryption.
pub fn room_modes(room: &MatrixRoom) -> String {
    let mut modes = String::new();

    if room.encryption_state().is_encrypted() {
        modes.push('e');
    }

    if !matches!(room.join_rule(), Some(JoinRule::Public) | None) {
        modes.push('i');
    }

    modes
}

/// Scrollback lines and status for a joined room's window.
///
/// Also does what the classic scrollback does on render: ask for older
/// messages when the screen isn't full, and update the read marker.
pub fn room_parts(
    chat: &ChatState,
    store: &mut ProgramStore,
    height: u16,
    notice: Option<(String, Style)>,
) -> (Vec<Line<'static>>, BxStatus) {
    let room_id = chat.id().to_owned();
    let channel = channel_name(chat.room(), store);
    let wins = windows(store);
    let refnum = wins.iter().find(|w| w.room_id == room_id).map(|w| w.refnum).unwrap_or(0);

    let app = &mut store.application;
    let me = app.settings.user_id.clone();
    let info = app.rooms.get_or_default(room_id.clone());
    let mut lines = vec![];
    let mut status = BxStatus {
        time: clock(),
        channel: Some(channel.clone()),
        modes: room_modes(chat.room()),
        act: act_list(&wins, refnum),
        window: refnum,
        ..Default::default()
    };

    let mut counts = UserCounts::default();
    for m in info.memberships.values().filter(|m| m.is_join()) {
        if is_op(m.power_level()) {
            counts.ops += 1;
        } else {
            counts.nonops += 1;
            if is_voice(m.power_level()) {
                counts.voice += 1;
            }
        }
    }
    status.users = Some(counts);

    if let Some(mine) = info.memberships.get(&me) {
        status.chanop = is_op(mine.power_level());
        status.voice = is_voice(mine.power_level());
    }

    let mut loaded = 0;
    if let Some(thread) = info.get_thread(chat.thread().map(|t| t.as_ref())) {
        loaded = thread.len();
        let ctx = FmtCtx { me: &me, info, channel: &channel };
        let recent: Vec<&Message> = thread.values().rev().take(height as usize).collect();

        for msg in recent.into_iter().rev() {
            lines.extend(format_message(msg, &ctx));
        }
    }

    if let Some((msg, style)) = notice {
        lines.push(notice_line(&msg, style));
    }

    if loaded < height as usize && !info.reached_timeline_start {
        app.need_load.need_messages(room_id.clone());
    }

    if app.settings.tunables.read_receipt_trigger.on_render(true, true) {
        let thread = info
            .get_thread(chat.thread().map(|t| t.as_ref()))
            .map(|t| t.1.clone())
            .unwrap_or(ReceiptThread::Main);
        info.fully_read(room_id, thread, &app.worker, &app.settings, &mut app.open_notifications);
    }

    info.draw_last = app.draw_curr;

    let info = store.application.rooms.get(chat.id());
    status.nick = own_nick(store, info);

    (lines, status)
}

/// The BitchX-like banner shown in the status window.
pub const BANNER: [&str; 6] = [
    r" ____  _ _       _     __  __",
    r"| __ )(_) |_ ___| |__  \ \/ /",
    r"|  _ \| | __/ __| '_ \  \  / ",
    r"| |_) | | || (__| | | | /  \ ",
    r"|____/|_|\__\___|_| |_|/_/\_\",
    r"",
];

/// Scrollback lines of the status window (window 1): banner, numbered
/// windows, hints.
pub fn status_window_lines(nick: &str, wins: &[BxWindow]) -> Vec<Line<'static>> {
    let colors = [
        Color::LightBlue,
        Color::LightBlue,
        Color::Cyan,
        Color::Cyan,
        Color::LightCyan,
        Color::Reset,
    ];
    let mut lines: Vec<Line<'static>> = BANNER
        .iter()
        .zip(colors)
        .map(|(l, c)| Line::from(sp(*l, bold(c))))
        .collect();

    lines.push(notice_line(
        &format!("iambitchx {}, a BitchX-style Matrix client", env!("CARGO_PKG_VERSION")),
        n(),
    ));
    lines.push(notice_line(&format!("You are {nick}"), n()));

    if wins.is_empty() {
        lines.push(notice_line("You are not on any channels", n()));
    } else {
        lines.push(notice_line("Windows:", w()));

        for win in wins {
            let mut spans = g();
            spans.push(sp("[", k()));
            spans.push(sp(win.refnum.to_string(), w()));
            spans.push(sp("] ", k()));
            spans.push(sp(win.title.clone(), fg(Color::LightCyan)));
            if win.unread {
                spans.push(sp(" (Act)", fg(Color::Yellow)));
            }
            lines.push(Line::from(spans));
        }
    }

    for hint in [
        "/join <#room> to join a room, /window <n> or Alt-<n> to switch windows",
        "/rooms to list your rooms, /help for all commands",
    ] {
        lines.push(notice_line(hint, n()));
    }

    lines
}

/// Scrollback lines and status for the status window (no room focused).
pub fn status_parts(
    store: &ProgramStore,
    notice: Option<(String, Style)>,
) -> (Vec<Line<'static>>, BxStatus) {
    let wins = windows(store);
    let nick = own_nick(store, None);
    let mut lines = status_window_lines(&nick, &wins);

    if let Some((msg, style)) = notice {
        lines.push(notice_line(&msg, style));
    }

    let status = BxStatus {
        time: clock(),
        nick,
        act: act_list(&wins, STATUS_REFNUM),
        window: STATUS_REFNUM,
        ..Default::default()
    };

    (lines, status)
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
    use serde_json::{Value, json};

    use super::*;
    use crate::tests::*;

    fn text(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn fmt(msg: &Message, me: &UserId) -> Vec<Line<'static>> {
        let info = mock_room();
        let ctx = FmtCtx { me, info: &info, channel: "#room1" };
        format_message(msg, &ctx)
    }

    fn content(c: RoomMessageEventContent, sender: &UserId) -> Message {
        mock_room1_message(c, sender.to_owned(), MSG2_KEY.clone())
    }

    fn state(kind: &str, key: &str, sender: &UserId, c: Value, prev: Option<Value>) -> Message {
        let mut v = json!({
            "type": kind,
            "state_key": key,
            "sender": sender,
            "event_id": "$st:example.com",
            "origin_server_ts": 1,
            "content": c,
        });
        if let Some(p) = prev {
            v["unsigned"] = json!({ "prev_content": p });
        }
        let ev: AnySyncStateEvent = serde_json::from_value(v).unwrap();
        Message::new(MessageEvent::State(Box::new(ev)), sender.to_owned(), MSG1_KEY.ts)
    }

    fn joined() -> Option<Value> {
        Some(json!({ "membership": "join" }))
    }

    /// `$G` = red `-`, white `:`, red `-`.
    fn assert_g(line: &Line<'static>) {
        let s = &line.spans;
        assert_eq!(s[0].content, "-");
        assert_eq!(s[0].style.fg, Some(Color::LightRed));
        assert_eq!(s[1].content, ":");
        assert_eq!(s[2].content, "-");
    }

    #[test]
    fn test_public_and_own() {
        let msg = content(RoomMessageEventContent::text_plain("hi"), &TEST_USER2);
        let l = fmt(&msg, &TEST_USER1);
        assert_eq!(text(&l), "<User 2> hi");
        assert_eq!(l[0].spans[0].style.fg, Some(Color::LightBlue));

        let own = content(RoomMessageEventContent::text_plain("me"), &TEST_USER1);
        let l = fmt(&own, &TEST_USER1);
        assert_eq!(text(&l), "<user1> me");
        assert_eq!(l[0].spans[0].style.fg, Some(Color::LightMagenta));

        let multi = content(RoomMessageEventContent::text_plain("a\nb"), &TEST_USER3);
        assert_eq!(text(&fmt(&multi, &TEST_USER1)), "<user3> a\n        b");
    }

    #[test]
    fn test_emote_and_notice() {
        let me = content(RoomMessageEventContent::emote_plain("waves"), &TEST_USER3);
        assert_eq!(text(&fmt(&me, &TEST_USER1)), "* user3 waves");

        let no = content(RoomMessageEventContent::notice_plain("bot says"), &TEST_USER3);
        assert_eq!(text(&fmt(&no, &TEST_USER1)), "-user3:#room1- bot says");
    }

    #[test]
    fn test_membership_lines() {
        let u3 = TEST_USER3.as_str();
        let join = state("m.room.member", u3, &TEST_USER3, json!({"membership":"join"}), None);
        let l = fmt(&join, &TEST_USER1);
        assert_g(&l[0]);
        assert_eq!(text(&l), format!("-:- user3 [{u3}] has joined #room1"));

        let part = state(
            "m.room.member",
            u3,
            &TEST_USER3,
            json!({"membership":"leave","reason":"bye"}),
            joined(),
        );
        let l = fmt(&part, &TEST_USER1);
        assert_g(&l[0]);
        assert_eq!(text(&l), format!("-:- user3 [{u3}] has left #room1 [bye]"));

        let kick = state(
            "m.room.member",
            u3,
            &TEST_USER2,
            json!({"membership":"leave","reason":"spam"}),
            joined(),
        );
        assert_eq!(
            text(&fmt(&kick, &TEST_USER1)),
            "-:- user3 was kicked off #room1 by User 2 (spam)"
        );

        let ban = state("m.room.member", u3, &TEST_USER2, json!({"membership":"ban"}), joined());
        assert_eq!(
            text(&fmt(&ban, &TEST_USER1)),
            "-:- mode/#room1 [+b user3] by User 2\n-:- user3 was kicked off #room1 by User 2"
        );
        let unban = state(
            "m.room.member",
            u3,
            &TEST_USER2,
            json!({"membership":"leave"}),
            Some(json!({"membership":"ban"})),
        );
        assert_eq!(text(&fmt(&unban, &TEST_USER1)), "-:- mode/#room1 [-b user3] by User 2");

        let nick = state(
            "m.room.member",
            u3,
            &TEST_USER3,
            json!({"membership":"join","displayname":"newnick"}),
            Some(json!({"membership":"join","displayname":"oldnick"})),
        );
        assert_eq!(text(&fmt(&nick, &TEST_USER1)), "-:- oldnick is now known as newnick");
    }

    #[test]
    fn test_topic_line() {
        let topic = state("m.room.topic", "", &TEST_USER2, json!({"topic":"hello"}), None);
        let l = fmt(&topic, &TEST_USER1);
        assert_g(&l[0]);
        assert_eq!(text(&l), "-:- Topic (#room1): changed by User 2: hello");
    }
}
