//! # BitchX slash-command dispatch
//!
//! Maps a parsed [`BitchxCommand`] onto iambitchx's existing machinery: iambitchx `:`
//! commands from `src/commands.rs`, [`IambAction`]s, window actions, or a
//! plain message. Everything here is pure, so the mapping is unit tested;
//! `ChatState::send_command` only executes the result.
use matrix_sdk::ruma::presence::PresenceState;
use modalkit::actions::CommandAction;

use crate::base::{HomeserverAction, MemberUpdateAction, RoomField};
use crate::bitchx::commands::{BitchxCommand, BitchxError, parse};
use crate::prelude::*;

/// Slash commands that iambitchx's message composer already understands (see
/// `src/message/compose.rs`). In BitchX mode they are sent as before.
const COMPOSE_COMMANDS: &[&str] = &[
    "h",
    "html",
    "p",
    "plain",
    "plaintext",
    "md",
    "markdown",
    "confetti",
    "fireworks",
    "hearts",
    "rainfall",
    "snowfall",
    "spaceinvaders",
];

/// One line of `/help` per command.
const HELP: &[(&str, &str)] = &[
    ("/msg <target> <text>", "send a message to a user or room"),
    ("/me, /action <text>", "send an emote"),
    ("/join, /j <room>", "join a room"),
    ("/part [room] [reason]", "leave a room"),
    ("/leave", "leave the current room"),
    ("/quit, /exit [reason]", "quit iambitchx"),
    ("/nick <name>", "set your display name"),
    ("/topic <topic>", "set the room topic"),
    ("/away [reason]", "set your presence to away"),
    ("/back", "set your presence to online"),
    ("/clear", "clear the screen"),
    ("/close", "close the current window"),
    ("/who, /names [room]", "show the member list"),
    ("/list", "show the room list"),
    ("/mode <room> [+o|-o|+b|-b <user>]", "show access or change a member"),
    ("/kick <user> [reason]", "kick a user"),
    ("/op, /deop <user>", "give or take operator (moderator, power level 50)"),
    ("/invite <user> [room]", "invite a user"),
    ("/query <target>", "open a direct chat"),
    ("/ignore, /unignore <user>", "ignore or unignore a user"),
    ("/window <n|room|next|prev>", "switch windows"),
    ("/next, /prev, /previous", "switch to the next or previous window"),
    ("/ctcp <target> <text>", "not available on Matrix"),
    ("/version", "show the iambitchx version"),
    ("/help", "show this help"),
];

/// What a BitchX command turns into.
#[derive(Clone, Debug, PartialEq)]
pub enum BxAction {
    /// Send this text to the current room via the normal compose path.
    Send(String),

    /// Run an iambitchx `:` command, reusing its implementation in `src/commands.rs`.
    Command(String),

    /// Run these actions in order.
    Actions(Vec<ProgramAction>),

    /// Show a short informational message.
    Info(String),

    /// Show a multi-line message in a pager.
    Pager(String),

    /// Show an error; nothing is sent.
    Error(String),
}

/// What `ChatState::send_command` should do with a `/` line.
#[derive(Clone, Debug, PartialEq)]
pub enum BxOutcome {
    /// Send this text as a message.
    Send(String),

    /// Clear the message bar and run these actions.
    Run(Vec<ProgramAction>),

    /// Show this error and keep the message bar as it is.
    Fail(String),
}

fn switch(target: &str) -> ProgramAction {
    WindowAction::Switch(OpenTarget::Name(target.to_string()), OpenFlags::NONE).into()
}

fn room(act: RoomAction) -> ProgramAction {
    IambAction::Room(act).into()
}

fn homeserver(act: HomeserverAction) -> ProgramAction {
    IambAction::Homeserver(act).into()
}

fn run(cmd: &str) -> ProgramAction {
    CommandAction::Run(cmd.to_string()).into()
}

fn focus(change: FocusChange) -> ProgramAction {
    WindowAction::Focus(change).into()
}

fn is_room_ref(s: &str) -> bool {
    s.starts_with('#') || s.starts_with('!')
}

/// Prefix `acts` with a switch to `target`, if one was given.
fn in_room(target: Option<String>, acts: Vec<ProgramAction>) -> BxAction {
    let mut all: Vec<ProgramAction> = target.iter().map(|t| switch(t)).collect();
    all.extend(acts);
    BxAction::Actions(all)
}

fn member(
    mua: MemberUpdateAction,
    user: String,
    reason: Option<String>,
    skip: bool,
) -> ProgramAction {
    room(RoomAction::MemberUpdate(mua, user, reason, skip))
}

/// `/mode <room> [modes]`: IRC channel modes mapped onto Matrix membership.
fn mode(target: String, modes: Option<String>) -> BxAction {
    if !is_room_ref(&target) {
        return BxAction::Error(format!("/mode: {target:?} is not a room (#alias or !id)"));
    }

    let Some(modes) = modes else {
        return in_room(Some(target), vec![room(RoomAction::Show(RoomField::Access))]);
    };

    let words: Vec<&str> = modes.split_whitespace().collect();
    let act = match words.as_slice() {
        ["+o", user] => member(MemberUpdateAction::Op, user.to_string(), None, true),
        ["-o", user] => member(MemberUpdateAction::Deop, user.to_string(), None, true),
        ["+b", user] => member(MemberUpdateAction::Ban, user.to_string(), None, false),
        ["-b", user] => member(MemberUpdateAction::Unban, user.to_string(), None, true),
        _ => {
            let msg = format!("/mode: unsupported mode {modes:?} (use +o, -o, +b or -b <user>)");
            return BxAction::Error(msg);
        },
    };

    in_room(Some(target), vec![act])
}

fn help() -> String {
    let width = HELP.iter().map(|(usage, _)| usage.len()).max().unwrap_or(0);
    let mut out = String::from("BitchX commands:\n");

    for (usage, desc) in HELP {
        out.push_str(&format!("  {usage:<width$}  {desc}\n"));
    }

    out
}

/// Map a parsed command onto iambitchx machinery.
pub fn dispatch(cmd: BitchxCommand) -> BxAction {
    match cmd {
        BitchxCommand::Msg { target, text } => {
            let send = IambAction::Send(SendAction::SubmitText(text)).into();
            BxAction::Actions(vec![switch(&target), send])
        },
        BitchxCommand::Me { text } | BitchxCommand::Action { text } => {
            BxAction::Send(format!("/me {text}"))
        },
        BitchxCommand::Join { room, key: None } => BxAction::Command(format!("join {room}")),
        BitchxCommand::Join { key: Some(_), .. } => {
            BxAction::Error("/join: Matrix rooms have no keys; use /join <room>".into())
        },
        // Matrix leave reasons are not supported by iambitchx's leave action.
        BitchxCommand::Part { room: target, .. } => {
            in_room(target, vec![room(RoomAction::Leave(true))])
        },
        BitchxCommand::Leave => BxAction::Actions(vec![room(RoomAction::Leave(true))]),
        BitchxCommand::Quit { .. } => BxAction::Command("quitall".into()),
        BitchxCommand::Nick { nick } => {
            let value = ProfileFieldValue::DisplayName(nick);
            BxAction::Actions(vec![homeserver(HomeserverAction::ProfileFieldSet(value))])
        },
        BitchxCommand::Topic { topic } => {
            BxAction::Actions(vec![room(RoomAction::Set(RoomField::Topic, topic))])
        },
        BitchxCommand::Away { reason } => {
            let act = HomeserverAction::SetPresence(PresenceState::Unavailable, reason);
            BxAction::Actions(vec![homeserver(act)])
        },
        BitchxCommand::Back => {
            let act = HomeserverAction::SetPresence(PresenceState::Online, None);
            BxAction::Actions(vec![homeserver(act)])
        },
        BitchxCommand::Clear => BxAction::Actions(vec![Action::RedrawScreen]),
        BitchxCommand::Close => {
            let target = WindowTarget::Single(FocusChange::Current);
            BxAction::Actions(vec![WindowAction::Close(target, CloseFlags::NONE).into()])
        },
        BitchxCommand::Who { target } | BitchxCommand::Names { target } => {
            match target {
                None => BxAction::Command("members".into()),
                Some(t) if is_room_ref(&t) => in_room(Some(t), vec![run("members")]),
                Some(t) => BxAction::Error(format!("/who: {t:?} is not a room (#alias or !id)")),
            }
        },
        BitchxCommand::List => BxAction::Command("rooms".into()),
        BitchxCommand::Mode { target, modes } => mode(target, modes),
        BitchxCommand::Kick { target, reason } => {
            BxAction::Actions(vec![member(MemberUpdateAction::Kick, target, reason, false)])
        },
        BitchxCommand::Op { target } => {
            BxAction::Actions(vec![member(MemberUpdateAction::Op, target, None, true)])
        },
        BitchxCommand::Deop { target } => {
            BxAction::Actions(vec![member(MemberUpdateAction::Deop, target, None, true)])
        },
        BitchxCommand::Invite { target, room: dest } => {
            match OwnedUserId::try_from(target.as_str()) {
                Ok(user) => in_room(dest, vec![room(RoomAction::InviteSend(user))]),
                Err(_) => BxAction::Error(format!("/invite: invalid user id {target:?}")),
            }
        },
        BitchxCommand::Query { target } => BxAction::Actions(vec![switch(&target)]),
        BitchxCommand::Ctcp { .. } => {
            BxAction::Error("/ctcp: CTCP does not exist on Matrix".into())
        },
        BitchxCommand::Ignore { pattern } => BxAction::Command(format!("ignore set {pattern}")),
        BitchxCommand::Unignore { pattern } => BxAction::Command(format!("ignore unset {pattern}")),
        BitchxCommand::Window { target: None } => {
            BxAction::Error("/window: expected a number, a room, next or prev".into())
        },
        BitchxCommand::Window { target: Some(t) } => {
            match t.parse::<usize>() {
                Ok(n) if n > 0 => {
                    BxAction::Actions(vec![focus(FocusChange::Offset(Count::Exact(n), false))])
                },
                Ok(_) => BxAction::Error("/window: windows are numbered from 1".into()),
                Err(_) => BxAction::Actions(vec![switch(&t)]),
            }
        },
        BitchxCommand::NextWindow => {
            let change = FocusChange::Direction1D(MoveDir1D::Next, Count::Exact(1), true);
            BxAction::Actions(vec![focus(change)])
        },
        BitchxCommand::PrevWindow => {
            let change = FocusChange::Direction1D(MoveDir1D::Previous, Count::Exact(1), true);
            BxAction::Actions(vec![focus(change)])
        },
        BitchxCommand::Version => {
            BxAction::Info(format!("iambitchx {} (BitchX frontend)", env!("CARGO_PKG_VERSION")))
        },
        BitchxCommand::Help => BxAction::Pager(help()),
    }
}

/// Handle a message-bar line starting with `/` in BitchX mode.
pub fn handle_input(line: &str) -> BxOutcome {
    let input = line.trim_start().trim_start_matches('/');

    let cmd = match parse(input) {
        Ok(None) => return BxOutcome::Send(line.to_string()),
        Ok(Some(cmd)) => cmd,
        Err(BitchxError::UnknownCommand(word))
            if COMPOSE_COMMANDS.contains(&word.to_lowercase().as_str()) =>
        {
            return BxOutcome::Send(line.to_string());
        },
        Err(e) => return BxOutcome::Fail(e.to_string()),
    };

    match dispatch(cmd) {
        BxAction::Send(text) => BxOutcome::Send(text),
        BxAction::Command(cmd) => BxOutcome::Run(vec![run(&cmd)]),
        BxAction::Actions(acts) => BxOutcome::Run(acts),
        BxAction::Info(msg) => {
            BxOutcome::Run(vec![Action::ShowInfoMessage(InfoMessage::Message(msg))])
        },
        BxAction::Pager(msg) => {
            BxOutcome::Run(vec![Action::ShowInfoMessage(InfoMessage::Pager(msg))])
        },
        BxAction::Error(msg) => BxOutcome::Fail(msg),
    }
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::user_id;
    use modalkit::editing::context::EditContext;

    use super::*;
    use crate::commands::setup_commands;

    fn s(v: &str) -> String {
        v.to_string()
    }

    fn acts(cmd: BitchxCommand) -> Vec<ProgramAction> {
        match dispatch(cmd) {
            BxAction::Actions(acts) => acts,
            other => panic!("expected actions, got {other:?}"),
        }
    }

    /// Run an iambitchx `:` command string through iambitchx's real command table.
    fn iamb_cmd(cmd: &str) -> Vec<ProgramAction> {
        let mut cmds = setup_commands(&Default::default()).unwrap();
        let ctx = EditContext::default();
        cmds.input_cmd(cmd, ctx)
            .unwrap_or_else(|e| panic!("{cmd:?} failed: {e:?}"))
            .into_iter()
            .map(|(act, _)| act)
            .collect()
    }

    fn command(cmd: BitchxCommand) -> Vec<ProgramAction> {
        match dispatch(cmd) {
            BxAction::Command(c) => iamb_cmd(&c),
            other => panic!("expected command, got {other:?}"),
        }
    }

    /// One sample of every [`BitchxCommand`] variant. The exhaustive match
    /// makes this fail to compile when a variant is added without a test.
    fn samples() -> Vec<BitchxCommand> {
        let all = vec![
            BitchxCommand::Msg { target: s("@bob:x.org"), text: s("hi") },
            BitchxCommand::Me { text: s("waves") },
            BitchxCommand::Action { text: s("waves") },
            BitchxCommand::Join { room: s("#a:x.org"), key: None },
            BitchxCommand::Part { room: None, reason: None },
            BitchxCommand::Leave,
            BitchxCommand::Quit { reason: None },
            BitchxCommand::Nick { nick: s("bob") },
            BitchxCommand::Topic { topic: s("t") },
            BitchxCommand::Away { reason: None },
            BitchxCommand::Back,
            BitchxCommand::Clear,
            BitchxCommand::Close,
            BitchxCommand::Who { target: None },
            BitchxCommand::Names { target: None },
            BitchxCommand::List,
            BitchxCommand::Mode { target: s("#a:x.org"), modes: None },
            BitchxCommand::Kick { target: s("@bob:x.org"), reason: None },
            BitchxCommand::Op { target: s("@bob:x.org") },
            BitchxCommand::Deop { target: s("@bob:x.org") },
            BitchxCommand::Invite { target: s("@bob:x.org"), room: None },
            BitchxCommand::Query { target: s("@bob:x.org") },
            BitchxCommand::Ctcp { target: s("bob"), text: s("VERSION") },
            BitchxCommand::Ignore { pattern: s("@bob:x.org") },
            BitchxCommand::Unignore { pattern: s("@bob:x.org") },
            BitchxCommand::Window { target: Some(s("2")) },
            BitchxCommand::NextWindow,
            BitchxCommand::PrevWindow,
            BitchxCommand::Version,
            BitchxCommand::Help,
        ];

        let mut seen = [false; 30];
        for cmd in &all {
            let i = match cmd {
                BitchxCommand::Msg { .. } => 0,
                BitchxCommand::Me { .. } => 1,
                BitchxCommand::Action { .. } => 2,
                BitchxCommand::Join { .. } => 3,
                BitchxCommand::Part { .. } => 4,
                BitchxCommand::Leave => 5,
                BitchxCommand::Quit { .. } => 6,
                BitchxCommand::Nick { .. } => 7,
                BitchxCommand::Topic { .. } => 8,
                BitchxCommand::Away { .. } => 9,
                BitchxCommand::Back => 10,
                BitchxCommand::Clear => 11,
                BitchxCommand::Close => 12,
                BitchxCommand::Who { .. } => 13,
                BitchxCommand::Names { .. } => 14,
                BitchxCommand::List => 15,
                BitchxCommand::Mode { .. } => 16,
                BitchxCommand::Kick { .. } => 17,
                BitchxCommand::Op { .. } => 18,
                BitchxCommand::Deop { .. } => 19,
                BitchxCommand::Invite { .. } => 20,
                BitchxCommand::Query { .. } => 21,
                BitchxCommand::Ctcp { .. } => 22,
                BitchxCommand::Ignore { .. } => 23,
                BitchxCommand::Unignore { .. } => 24,
                BitchxCommand::Window { .. } => 25,
                BitchxCommand::NextWindow => 26,
                BitchxCommand::PrevWindow => 27,
                BitchxCommand::Version => 28,
                BitchxCommand::Help => 29,
            };
            seen[i] = true;
        }
        assert!(seen.iter().all(|s| *s), "a variant has no sample");

        all
    }

    #[test]
    fn test_bitchx_dispatch_every_variant_is_concrete() {
        for cmd in samples() {
            match dispatch(cmd.clone()) {
                BxAction::Send(text) => assert!(!text.is_empty(), "{cmd:?}"),
                BxAction::Actions(acts) => assert!(!acts.is_empty(), "{cmd:?}"),
                BxAction::Command(c) => assert!(!iamb_cmd(&c).is_empty(), "{cmd:?}"),
                BxAction::Info(msg) | BxAction::Pager(msg) | BxAction::Error(msg) => {
                    assert!(!msg.is_empty(), "{cmd:?}")
                },
            }

            // Every sample also survives the full input path.
            match handle_input(&format!("/{}", sample_line(&cmd))) {
                BxOutcome::Send(t) => assert!(!t.is_empty()),
                BxOutcome::Run(acts) => assert!(!acts.is_empty()),
                BxOutcome::Fail(msg) => assert!(!msg.is_empty()),
            }
        }
    }

    /// A typed line that parses back to the sample.
    fn sample_line(cmd: &BitchxCommand) -> String {
        let line = match cmd {
            BitchxCommand::Msg { .. } => "msg @bob:x.org hi",
            BitchxCommand::Me { .. } => "me waves",
            BitchxCommand::Action { .. } => "action waves",
            BitchxCommand::Join { .. } => "join #a:x.org",
            BitchxCommand::Part { .. } => "part",
            BitchxCommand::Leave => "leave",
            BitchxCommand::Quit { .. } => "quit",
            BitchxCommand::Nick { .. } => "nick bob",
            BitchxCommand::Topic { .. } => "topic t",
            BitchxCommand::Away { .. } => "away",
            BitchxCommand::Back => "back",
            BitchxCommand::Clear => "clear",
            BitchxCommand::Close => "close",
            BitchxCommand::Who { .. } => "who",
            BitchxCommand::Names { .. } => "names",
            BitchxCommand::List => "list",
            BitchxCommand::Mode { .. } => "mode #a:x.org",
            BitchxCommand::Kick { .. } => "kick @bob:x.org",
            BitchxCommand::Op { .. } => "op @bob:x.org",
            BitchxCommand::Deop { .. } => "deop @bob:x.org",
            BitchxCommand::Invite { .. } => "invite @bob:x.org",
            BitchxCommand::Query { .. } => "query @bob:x.org",
            BitchxCommand::Ctcp { .. } => "ctcp bob VERSION",
            BitchxCommand::Ignore { .. } => "ignore @bob:x.org",
            BitchxCommand::Unignore { .. } => "unignore @bob:x.org",
            BitchxCommand::Window { .. } => "window 2",
            BitchxCommand::NextWindow => "next",
            BitchxCommand::PrevWindow => "prev",
            BitchxCommand::Version => "version",
            BitchxCommand::Help => "help",
        };
        assert_eq!(parse(line), Ok(Some(cmd.clone())), "{line:?}");
        line.to_string()
    }

    #[test]
    fn test_bitchx_dispatch_messages() {
        assert_eq!(
            acts(BitchxCommand::Msg { target: s("@bob:x.org"), text: s("hi there") }),
            vec![
                switch("@bob:x.org"),
                IambAction::Send(SendAction::SubmitText(s("hi there"))).into(),
            ]
        );
        let me = BitchxCommand::Me { text: s("waves") };
        assert_eq!(dispatch(me), BxAction::Send(s("/me waves")));
        let action = BitchxCommand::Action { text: s("waves") };
        assert_eq!(dispatch(action), BxAction::Send(s("/me waves")));
        assert_eq!(acts(BitchxCommand::Query { target: s("@bob:x.org") }), vec![switch(
            "@bob:x.org"
        )]);
    }

    #[test]
    fn test_bitchx_dispatch_iamb_commands() {
        let join = BitchxCommand::Join { room: s("#a:x.org"), key: None };
        assert_eq!(command(join), vec![
            WindowAction::Switch(OpenTarget::Name(s("#a:x.org")), OpenFlags::FORCE).into()
        ]);
        assert_eq!(command(BitchxCommand::List), iamb_cmd("rooms"));
        assert_eq!(command(BitchxCommand::Who { target: None }), iamb_cmd("members"));
        assert_eq!(command(BitchxCommand::Names { target: None }), iamb_cmd("members"));
        assert_eq!(command(BitchxCommand::Quit { reason: Some(s("bye")) }), iamb_cmd("qa"));
        assert_eq!(command(BitchxCommand::Ignore { pattern: s("@bob:x.org") }), vec![homeserver(
            HomeserverAction::AccountSet(AccountField::Ignore, s("@bob:x.org"))
        )]);
        assert_eq!(command(BitchxCommand::Unignore { pattern: s("@bob:x.org") }), vec![
            homeserver(HomeserverAction::AccountUnset(AccountField::Ignore, Some(s("@bob:x.org"))))
        ]);
        assert!(matches!(
            dispatch(BitchxCommand::Join { room: s("#a:x.org"), key: Some(s("k")) }),
            BxAction::Error(_)
        ));
    }

    #[test]
    fn test_bitchx_dispatch_room_actions() {
        let leave = room(RoomAction::Leave(true));
        assert_eq!(acts(BitchxCommand::Leave), vec![leave.clone()]);
        assert_eq!(acts(BitchxCommand::Part { room: None, reason: Some(s("bye")) }), vec![
            leave.clone()
        ]);
        assert_eq!(acts(BitchxCommand::Part { room: Some(s("#a:x.org")), reason: None }), vec![
            switch("#a:x.org"),
            leave
        ]);
        assert_eq!(acts(BitchxCommand::Topic { topic: s("new topic") }), vec![room(
            RoomAction::Set(RoomField::Topic, s("new topic"))
        )]);
        assert_eq!(acts(BitchxCommand::Who { target: Some(s("#a:x.org")) }), vec![
            switch("#a:x.org"),
            run("members")
        ]);
        assert!(matches!(
            dispatch(BitchxCommand::Who { target: Some(s("bob")) }),
            BxAction::Error(_)
        ));

        let bob = user_id!("@bob:x.org").to_owned();
        assert_eq!(acts(BitchxCommand::Invite { target: s("@bob:x.org"), room: None }), vec![
            room(RoomAction::InviteSend(bob.clone()))
        ]);
        assert_eq!(
            acts(BitchxCommand::Invite { target: s("@bob:x.org"), room: Some(s("#a:x.org")) }),
            vec![switch("#a:x.org"), room(RoomAction::InviteSend(bob))]
        );
        assert!(matches!(
            dispatch(BitchxCommand::Invite { target: s("bob"), room: None }),
            BxAction::Error(_)
        ));
    }

    #[test]
    fn test_bitchx_dispatch_member_updates() {
        let kick = BitchxCommand::Kick { target: s("@bob:x.org"), reason: Some(s("spam")) };
        assert_eq!(acts(kick), vec![member(
            MemberUpdateAction::Kick,
            s("@bob:x.org"),
            Some(s("spam")),
            false
        )]);
        assert_eq!(acts(BitchxCommand::Op { target: s("@bob:x.org") }), vec![member(
            MemberUpdateAction::Op,
            s("@bob:x.org"),
            None,
            true
        )]);
        assert_eq!(acts(BitchxCommand::Deop { target: s("@bob:x.org") }), vec![member(
            MemberUpdateAction::Deop,
            s("@bob:x.org"),
            None,
            true
        )]);

        let mode = |modes: Option<&str>| {
            BitchxCommand::Mode { target: s("#a:x.org"), modes: modes.map(s) }
        };
        assert_eq!(acts(mode(None)), vec![
            switch("#a:x.org"),
            room(RoomAction::Show(RoomField::Access))
        ]);
        assert_eq!(acts(mode(Some("+o @bob:x.org"))), vec![
            switch("#a:x.org"),
            member(MemberUpdateAction::Op, s("@bob:x.org"), None, true)
        ]);
        assert_eq!(acts(mode(Some("-o @bob:x.org"))), vec![
            switch("#a:x.org"),
            member(MemberUpdateAction::Deop, s("@bob:x.org"), None, true)
        ]);
        assert_eq!(acts(mode(Some("+b @bob:x.org"))), vec![
            switch("#a:x.org"),
            member(MemberUpdateAction::Ban, s("@bob:x.org"), None, false)
        ]);
        assert_eq!(acts(mode(Some("-b @bob:x.org"))), vec![
            switch("#a:x.org"),
            member(MemberUpdateAction::Unban, s("@bob:x.org"), None, true)
        ]);
        assert!(matches!(dispatch(mode(Some("+k secret"))), BxAction::Error(_)));
        let user_mode = BitchxCommand::Mode { target: s("bob"), modes: Some(s("+i")) };
        assert!(matches!(dispatch(user_mode), BxAction::Error(_)));
    }

    #[test]
    fn test_bitchx_dispatch_account() {
        assert_eq!(acts(BitchxCommand::Nick { nick: s("Bob B") }), vec![homeserver(
            HomeserverAction::ProfileFieldSet(ProfileFieldValue::DisplayName(s("Bob B")))
        )]);
        assert_eq!(acts(BitchxCommand::Away { reason: Some(s("lunch")) }), vec![homeserver(
            HomeserverAction::SetPresence(PresenceState::Unavailable, Some(s("lunch")))
        )]);
        assert_eq!(acts(BitchxCommand::Back), vec![homeserver(HomeserverAction::SetPresence(
            PresenceState::Online,
            None
        ))]);
    }

    #[test]
    fn test_bitchx_dispatch_windows() {
        assert_eq!(acts(BitchxCommand::Clear), vec![Action::RedrawScreen]);
        assert_eq!(acts(BitchxCommand::Close), vec![
            WindowAction::Close(WindowTarget::Single(FocusChange::Current), CloseFlags::NONE)
                .into()
        ]);
        assert_eq!(acts(BitchxCommand::Window { target: Some(s("3")) }), vec![focus(
            FocusChange::Offset(Count::Exact(3), false)
        )]);
        assert_eq!(acts(BitchxCommand::Window { target: Some(s("#a:x.org")) }), vec![switch(
            "#a:x.org"
        )]);
        assert!(matches!(
            dispatch(BitchxCommand::Window { target: Some(s("0")) }),
            BxAction::Error(_)
        ));
        assert!(matches!(dispatch(BitchxCommand::Window { target: None }), BxAction::Error(_)));
        assert_eq!(acts(BitchxCommand::NextWindow), vec![focus(FocusChange::Direction1D(
            MoveDir1D::Next,
            Count::Exact(1),
            true
        ))]);
        assert_eq!(acts(BitchxCommand::PrevWindow), vec![focus(FocusChange::Direction1D(
            MoveDir1D::Previous,
            Count::Exact(1),
            true
        ))]);
    }

    #[test]
    fn test_bitchx_dispatch_info() {
        let BxAction::Info(version) = dispatch(BitchxCommand::Version) else {
            panic!("expected info");
        };
        assert!(version.contains(env!("CARGO_PKG_VERSION")));

        let BxAction::Pager(help) = dispatch(BitchxCommand::Help) else {
            panic!("expected pager");
        };
        for name in crate::bitchx::commands::COMMAND_NAMES {
            assert!(help.contains(&format!("/{name}")), "/help is missing /{name}");
        }

        assert!(matches!(
            dispatch(BitchxCommand::Ctcp { target: s("bob"), text: s("VERSION") }),
            BxAction::Error(_)
        ));
    }

    #[test]
    fn test_bitchx_handle_input() {
        // Empty commands and iambitchx's own compose commands are sent as-is.
        assert_eq!(handle_input("/"), BxOutcome::Send(s("/")));
        assert_eq!(handle_input("/  "), BxOutcome::Send(s("/  ")));
        assert_eq!(handle_input("/html <b>hi</b>"), BxOutcome::Send(s("/html <b>hi</b>")));
        assert_eq!(handle_input("/Confetti yay"), BxOutcome::Send(s("/Confetti yay")));

        // Emotes go through the normal send path.
        assert_eq!(handle_input("/me waves"), BxOutcome::Send(s("/me waves")));

        // Parse errors fail without sending.
        assert_eq!(handle_input("/bogus"), BxOutcome::Fail(s("Unknown command: /bogus")));
        assert!(matches!(handle_input("/msg"), BxOutcome::Fail(_)));

        // Commands run actions.
        assert_eq!(handle_input("/join #a:x.org"), BxOutcome::Run(vec![run("join #a:x.org")]));
        assert_eq!(handle_input("/leave"), BxOutcome::Run(vec![room(RoomAction::Leave(true))]));
        let BxOutcome::Run(info) = handle_input("/version") else {
            panic!("expected run");
        };
        assert!(matches!(info.as_slice(), [Action::ShowInfoMessage(InfoMessage::Message(_))]));
        let BxOutcome::Run(help) = handle_input("/HELP") else {
            panic!("expected run");
        };
        assert!(matches!(help.as_slice(), [Action::ShowInfoMessage(InfoMessage::Pager(_))]));
    }
}
