//! # BitchX slash-command parser
//!
//! Pure parsing of the text typed after a leading `/` in the message bar.
//! Nothing here knows about Matrix; dispatch happens elsewhere.
use std::fmt;

use crate::bitchx::channels::{ListArgs, parse_list_args};

/// A parsed BitchX/IRC-style slash command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BitchxCommand {
    Msg {
        target: String,
        text: String,
    },
    Me {
        text: String,
    },
    Action {
        text: String,
    },
    Join {
        room: String,
        key: Option<String>,
    },
    Part {
        room: Option<String>,
        reason: Option<String>,
    },
    Leave,
    Quit {
        reason: Option<String>,
    },
    Nick {
        nick: String,
    },
    Topic {
        topic: String,
    },
    Away {
        reason: Option<String>,
    },
    Back,
    Clear,
    Close,
    Who {
        target: Option<String>,
    },
    Whois {
        target: String,
    },
    /// `/channels` (hidden alias `/rooms`): the joined channels.
    Channels,
    Names {
        target: Option<String>,
    },
    /// `/list [server] [-s text]`: the public channel directory.
    List(ListArgs),
    Mode {
        target: String,
        modes: Option<String>,
    },
    Kick {
        target: String,
        reason: Option<String>,
    },
    Op {
        target: String,
    },
    Deop {
        target: String,
    },
    Invite {
        target: String,
        room: Option<String>,
    },
    /// `/query nick`; `/query` alone ends the current query.
    Query {
        target: Option<String>,
    },
    Ctcp {
        target: String,
        text: String,
    },
    Ignore {
        pattern: String,
    },
    Unignore {
        pattern: String,
    },
    Window {
        target: Option<String>,
    },
    NextWindow,
    PrevWindow,
    /// `/window split on|off|toggle`: the top status bar.
    WindowSplit(Option<bool>),
    Version,
    Help,
    /// `/encrypt`: enable megolm in the current room (asks y/n first).
    Encrypt,
    /// `/verify [accept|start|confirm|mismatch|cancel] <user> [device]`.
    Verify(VerifyCmd),
    /// `/devices [user]`: list devices with verified flags.
    Devices {
        user: Option<String>,
    },
    /// `/trust`: own cross-signing status.
    Trust,
    /// `/recover <recovery key>`: import secrets from secure backup.
    Recover {
        key: Secret,
    },
    /// `/set`: list all variables; `/set NAME`: show one; `/set NAME value`: set it.
    Set {
        name: Option<String>,
        value: Option<String>,
    },
}

/// `/verify` subcommands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerifyCmd {
    /// `/verify`: list pending verifications.
    List,
    /// `flow` picks one of several pending flows with `user` (F44).
    Accept {
        user: String,
        flow: Option<String>,
    },
    Start {
        user: String,
        device: Option<String>,
    },
    Confirm {
        user: String,
        flow: Option<String>,
    },
    Mismatch {
        user: String,
        flow: Option<String>,
    },
    Cancel {
        user: String,
        flow: Option<String>,
    },
}

/// A secret typed by the user (recovery key). Never printed: `Debug` is
/// redacted and there is no `Display`.
#[derive(Clone, Eq, PartialEq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Secret(s.into())
    }

    /// The secret itself, for handing to the SDK only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Commands whose arguments are secrets: masked in history and on screen.
pub const SECRET_COMMANDS: &[&str] = &["recover"];

/// If `line` is `/recover ...` (any case), the byte offset where the secret
/// argument starts (after the command word and one separator).
pub fn secret_offset(line: &str) -> Option<usize> {
    let rest = line.strip_prefix('/')?;
    let lead = rest.len() - rest.trim_start().len();
    let rest = &rest[lead..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    if !SECRET_COMMANDS.contains(&rest[..end].to_lowercase().as_str()) {
        return None;
    }
    let sep = rest[end..].chars().next()?;
    Some(1 + lead + end + sep.len_utf8())
}

/// For the screen while typing: every char of the secret becomes `*`.
pub fn mask_secret(line: &str) -> String {
    match secret_offset(line) {
        Some(off) => format!("{}{}", &line[..off], "*".repeat(line[off..].chars().count())),
        None => line.to_string(),
    }
}

/// For history: the secret becomes a fixed `********` (no length leak).
pub fn redact_secret(line: &str) -> String {
    match secret_offset(line) {
        Some(off) => format!("{}********", &line[..off]),
        None => line.to_string(),
    }
}

/// Why a slash command could not be parsed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BitchxError {
    UnknownCommand(String),
    MissingArgument { command: &'static str, argument: &'static str },
    InvalidArgument { command: &'static str, argument: String },
}

impl fmt::Display for BitchxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BitchxError::UnknownCommand(cmd) => {
                write!(f, "Unknown command: {}", cmd.to_uppercase())
            },
            BitchxError::MissingArgument { command, argument } => {
                write!(f, "/{command}: missing argument <{argument}>")
            },
            BitchxError::InvalidArgument { command, argument } => {
                write!(f, "/{command}: invalid argument {argument:?}")
            },
        }
    }
}

impl std::error::Error for BitchxError {}

/// Every command word understood by [`parse`], including aliases, for `/help`.
pub const COMMAND_NAMES: &[&str] = &[
    "action",
    "away",
    "back",
    "channels",
    "clear",
    "close",
    "ctcp",
    "deop",
    "devices",
    "encrypt",
    "exit",
    "help",
    "ignore",
    "invite",
    "j",
    "join",
    "kick",
    "leave",
    "list",
    "me",
    "mircslap",
    "mircslaps",
    "mode",
    "msg",
    "names",
    "next",
    "nick",
    "op",
    "part",
    "prev",
    "previous",
    "query",
    "quit",
    "recover",
    "set",
    "slap",
    "slaps",
    "topic",
    "trust",
    "unignore",
    "verify",
    "version",
    "who",
    "whois",
    "window",
];

/// The original BitchX slap (script/menu.bx:1385).
pub fn slap_text(nick: &str) -> String {
    format!("slaps {nick} around a bit with a substantially large Brown Cow...")
}

/// The classic mIRC slap.
pub fn mircslap_text(nick: &str) -> String {
    format!("slaps {nick} around a bit with a large trout")
}

/// Split off the first whitespace-delimited word, returning it and the rest
/// with only the separating whitespace removed (so bodies keep their spaces).
fn split_word(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();

    if s.is_empty() {
        return None;
    }

    let end = s.find(char::is_whitespace).unwrap_or(s.len());

    Some((&s[..end], s[end..].trim_start()))
}

/// Whether a token looks like a Matrix room reference (alias or ID).
fn is_room_ref(s: &str) -> bool {
    s.starts_with('#') || s.starts_with('!')
}

/// Argument cursor for a single command.
struct Args<'a> {
    command: &'static str,
    rest: &'a str,
}

impl<'a> Args<'a> {
    fn word(&mut self) -> Option<String> {
        let (word, rest) = split_word(self.rest)?;
        self.rest = rest;
        Some(word.to_string())
    }

    fn required_word(&mut self, argument: &'static str) -> Result<String, BitchxError> {
        self.word()
            .ok_or(BitchxError::MissingArgument { command: self.command, argument })
    }

    fn peek_word(&self) -> Option<&'a str> {
        split_word(self.rest).map(|(word, _)| word)
    }

    /// The remainder of the line, untouched, or `None` if only whitespace.
    fn rest(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.rest);

        if rest.trim().is_empty() {
            None
        } else {
            Some(rest.to_string())
        }
    }

    fn required_rest(&mut self, argument: &'static str) -> Result<String, BitchxError> {
        self.rest()
            .ok_or(BitchxError::MissingArgument { command: self.command, argument })
    }

    /// Fail if any argument is left over.
    fn finish(self) -> Result<(), BitchxError> {
        let rest = self.rest.trim();

        if rest.is_empty() {
            Ok(())
        } else {
            Err(BitchxError::InvalidArgument { command: self.command, argument: rest.to_string() })
        }
    }

    fn none(self, cmd: BitchxCommand) -> Result<BitchxCommand, BitchxError> {
        self.finish().map(|()| cmd)
    }
}

/// `/verify` with no args lists; otherwise `<sub> <user> [device|flow]`.
fn parse_verify(args: &mut Args<'_>) -> Result<VerifyCmd, BitchxError> {
    let Some(sub) = args.word() else {
        return Ok(VerifyCmd::List);
    };
    let sub = sub.to_lowercase();
    let cmd = match sub.as_str() {
        "list" => VerifyCmd::List,
        "accept" => {
            VerifyCmd::Accept {
                user: args.required_word("user")?,
                flow: args.word(),
            }
        },
        "start" => {
            let user = args.required_word("user")?;
            VerifyCmd::Start { user, device: args.word() }
        },
        "confirm" | "yes" => {
            VerifyCmd::Confirm {
                user: args.required_word("user")?,
                flow: args.word(),
            }
        },
        "mismatch" | "no" => {
            VerifyCmd::Mismatch {
                user: args.required_word("user")?,
                flow: args.word(),
            }
        },
        "cancel" => {
            VerifyCmd::Cancel {
                user: args.required_word("user")?,
                flow: args.word(),
            }
        },
        _ => return Err(BitchxError::InvalidArgument { command: "verify", argument: sub }),
    };
    let rest = std::mem::take(&mut args.rest).trim().to_string();
    if rest.is_empty() {
        Ok(cmd)
    } else {
        Err(BitchxError::InvalidArgument { command: "verify", argument: rest })
    }
}

/// Parse the text typed after the leading `/`.
///
/// Returns `Ok(None)` for empty/whitespace-only input, which the caller
/// should treat as a normal message.
pub fn parse(input: &str) -> Result<Option<BitchxCommand>, BitchxError> {
    let Some((word, rest)) = split_word(input) else {
        return Ok(None);
    };

    let lower = word.to_lowercase();
    let canonical: &'static str = match lower.as_str() {
        "exit" => "quit",
        "j" => "join",
        "previous" => "prev",
        "rooms" => "channels",
        other => {
            match COMMAND_NAMES.iter().copied().find(|name| *name == other) {
                Some(name) => name,
                None => return Err(BitchxError::UnknownCommand(word.to_string())),
            }
        },
    };

    let mut args = Args { command: canonical, rest };

    let cmd = match canonical {
        "msg" => {
            let target = args.required_word("target")?;
            let text = args.required_rest("text")?;
            BitchxCommand::Msg { target, text }
        },
        "me" => BitchxCommand::Me { text: args.required_rest("text")? },
        "action" => BitchxCommand::Action { text: args.required_rest("text")? },
        // BitchX menu.bx "Slap" action (script/menu.bx:1385), sent as /me.
        "slap" | "slaps" => {
            let nick = args.required_word("nick")?;
            BitchxCommand::Me { text: slap_text(&nick) }
        },
        "mircslap" | "mircslaps" => {
            let nick = args.required_word("nick")?;
            BitchxCommand::Me { text: mircslap_text(&nick) }
        },
        "join" => {
            let room = args.required_word("channel")?;
            let key = args.word();
            args.finish()?;
            BitchxCommand::Join { room, key }
        },
        "part" => {
            // IRC-style: `/part [#chan] [reason]`; a first word that is not a
            // channel reference starts the reason.
            let room = match args.peek_word() {
                Some(w) if is_room_ref(w) => args.word(),
                _ => None,
            };
            let reason = args.rest();
            BitchxCommand::Part { room, reason }
        },
        // BitchX: /leave is an alias of /part, so it takes the same arguments.
        "leave" if args.peek_word().is_none() => BitchxCommand::Leave,
        "leave" => {
            let room = match args.peek_word() {
                Some(w) if is_room_ref(w) => args.word(),
                _ => None,
            };
            let reason = args.rest();
            BitchxCommand::Part { room, reason }
        },
        "quit" => BitchxCommand::Quit { reason: args.rest() },
        "nick" => BitchxCommand::Nick { nick: args.required_rest("nick")? },
        "topic" => BitchxCommand::Topic { topic: args.required_rest("topic")? },
        "away" => BitchxCommand::Away { reason: args.rest() },
        "back" => args.none(BitchxCommand::Back)?,
        "clear" => args.none(BitchxCommand::Clear)?,
        "close" => args.none(BitchxCommand::Close)?,
        "who" => {
            let target = args.word();
            args.finish()?;
            BitchxCommand::Who { target }
        },
        "names" => {
            let target = args.word();
            args.finish()?;
            BitchxCommand::Names { target }
        },
        "list" => {
            let rest = args.rest().unwrap_or_default();
            match parse_list_args(&rest) {
                Ok(l) => BitchxCommand::List(l),
                Err(_) => {
                    return Err(BitchxError::InvalidArgument {
                        command: "list",
                        argument: rest.trim().to_string(),
                    });
                },
            }
        },
        "channels" => args.none(BitchxCommand::Channels)?,
        "whois" => {
            let target = args.required_word("nick")?;
            args.finish()?;
            BitchxCommand::Whois { target }
        },
        "mode" => {
            let target = args.required_word("target")?;
            let modes = args.rest();
            BitchxCommand::Mode { target, modes }
        },
        "kick" => {
            let target = args.required_word("target")?;
            let reason = args.rest();
            BitchxCommand::Kick { target, reason }
        },
        "op" => {
            let target = args.required_word("target")?;
            args.finish()?;
            BitchxCommand::Op { target }
        },
        "deop" => {
            let target = args.required_word("target")?;
            args.finish()?;
            BitchxCommand::Deop { target }
        },
        "invite" => {
            let target = args.required_word("target")?;
            let room = args.word();
            args.finish()?;
            BitchxCommand::Invite { target, room }
        },
        "query" => {
            let target = args.word();
            args.finish()?;
            BitchxCommand::Query { target }
        },
        "set" => {
            let name = args.word();
            // The value keeps its spaces (`TIMESTAMP_STRING` has a trailing one).
            let value = args.rest();
            BitchxCommand::Set { name, value }
        },
        "ctcp" => {
            let target = args.required_word("target")?;
            let text = args.required_rest("text")?;
            BitchxCommand::Ctcp { target, text }
        },
        "ignore" => {
            let pattern = args.required_word("pattern")?;
            args.finish()?;
            BitchxCommand::Ignore { pattern }
        },
        "unignore" => {
            let pattern = args.required_word("pattern")?;
            args.finish()?;
            BitchxCommand::Unignore { pattern }
        },
        "window" => {
            let target = args.word();
            if target.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("split")) {
                let mode = args.required_word("on|off|toggle")?;
                args.finish()?;
                return Ok(Some(match mode.to_lowercase().as_str() {
                    "on" => BitchxCommand::WindowSplit(Some(true)),
                    "off" => BitchxCommand::WindowSplit(Some(false)),
                    "toggle" => BitchxCommand::WindowSplit(None),
                    _ => {
                        return Err(BitchxError::InvalidArgument {
                            command: "window",
                            argument: mode,
                        });
                    },
                }));
            }
            args.finish()?;

            // BitchX-style `/window next` and `/window prev`.
            match target.as_deref().map(str::to_lowercase).as_deref() {
                Some("next") => BitchxCommand::NextWindow,
                Some("prev" | "previous") => BitchxCommand::PrevWindow,
                _ => BitchxCommand::Window { target },
            }
        },
        "next" => args.none(BitchxCommand::NextWindow)?,
        "prev" => args.none(BitchxCommand::PrevWindow)?,
        "version" => args.none(BitchxCommand::Version)?,
        "help" => args.none(BitchxCommand::Help)?,
        "encrypt" => args.none(BitchxCommand::Encrypt)?,
        "trust" => args.none(BitchxCommand::Trust)?,
        "devices" => {
            let user = args.word();
            args.finish()?;
            BitchxCommand::Devices { user }
        },
        "recover" => {
            // The whole remainder: recovery keys are space-separated groups.
            // Errors must never echo it, so only `required_rest` is used.
            let key = args.required_rest("recovery key")?;
            BitchxCommand::Recover { key: Secret::new(key.trim()) }
        },
        "verify" => BitchxCommand::Verify(parse_verify(&mut args)?),
        _ => return Err(BitchxError::UnknownCommand(word.to_string())),
    };

    Ok(Some(cmd))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &str) -> BitchxCommand {
        parse(input)
            .unwrap_or_else(|e| panic!("{input:?} failed: {e}"))
            .expect("command")
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    fn missing(command: &'static str, argument: &'static str) -> BitchxError {
        BitchxError::MissingArgument { command, argument }
    }

    fn invalid(command: &'static str, argument: &str) -> BitchxError {
        BitchxError::InvalidArgument { command, argument: s(argument) }
    }

    #[test]
    fn test_bitchx_empty_input() {
        assert_eq!(parse(""), Ok(None));
        assert_eq!(parse("   "), Ok(None));
        assert_eq!(parse(" \t \n"), Ok(None));
    }

    #[test]
    fn test_bitchx_unknown_command() {
        assert_eq!(parse("frobnicate x"), Err(BitchxError::UnknownCommand(s("frobnicate"))));
        assert_eq!(parse("Frob"), Err(BitchxError::UnknownCommand(s("Frob"))));
        // Prefixes of known commands are not commands.
        assert_eq!(parse("ms hi"), Err(BitchxError::UnknownCommand(s("ms"))));
    }

    #[test]
    fn test_bitchx_case_insensitive() {
        let expect = BitchxCommand::Msg { target: s("@bob:x.org"), text: s("hi") };
        assert_eq!(ok("msg @bob:x.org hi"), expect);
        assert_eq!(ok("MSG @bob:x.org hi"), expect);
        assert_eq!(ok("Msg @bob:x.org hi"), expect);
        assert_eq!(ok("HELP"), BitchxCommand::Help);
        assert_eq!(ok("Window NEXT"), BitchxCommand::NextWindow);
    }

    #[test]
    fn test_bitchx_msg() {
        assert_eq!(ok("msg bob hello   there  world"), BitchxCommand::Msg {
            target: s("bob"),
            text: s("hello   there  world"),
        });
        assert_eq!(parse("msg"), Err(missing("msg", "target")));
        assert_eq!(parse("msg bob"), Err(missing("msg", "text")));
        assert_eq!(parse("msg bob    "), Err(missing("msg", "text")));
    }

    #[test]
    fn test_bitchx_me_action() {
        assert_eq!(ok("me waves  at you"), BitchxCommand::Me { text: s("waves  at you") });
        assert_eq!(ok("action waves"), BitchxCommand::Action { text: s("waves") });
        assert_eq!(parse("me"), Err(missing("me", "text")));
        assert_eq!(parse("action  "), Err(missing("action", "text")));
    }

    #[test]
    fn test_bitchx_join() {
        assert_eq!(ok("join #rust:matrix.org"), BitchxCommand::Join {
            room: s("#rust:matrix.org"),
            key: None,
        });
        assert_eq!(ok("join #a:b secret"), BitchxCommand::Join {
            room: s("#a:b"),
            key: Some(s("secret")),
        });
        assert_eq!(ok("j #a:b"), BitchxCommand::Join { room: s("#a:b"), key: None });
        assert_eq!(parse("join"), Err(missing("join", "channel")));
        assert_eq!(parse("join #a:b key extra"), Err(invalid("join", "extra")));
    }

    #[test]
    fn test_bitchx_part_leave() {
        assert_eq!(ok("part"), BitchxCommand::Part { room: None, reason: None });
        assert_eq!(ok("part #a:b"), BitchxCommand::Part { room: Some(s("#a:b")), reason: None });
        assert_eq!(ok("part !id:b gone  fishing"), BitchxCommand::Part {
            room: Some(s("!id:b")),
            reason: Some(s("gone  fishing")),
        });
        assert_eq!(ok("part gone fishing"), BitchxCommand::Part {
            room: None,
            reason: Some(s("gone fishing")),
        });
        assert_eq!(ok("leave"), BitchxCommand::Leave);
        assert_eq!(ok("LEAVE"), BitchxCommand::Leave);
        let slap = BitchxCommand::Me {
            text: s("slaps armin around a bit with a substantially large Brown Cow..."),
        };
        assert_eq!(ok("slap armin"), slap);
        assert_eq!(ok("slaps armin"), slap);
        assert!(parse("slap").is_err());
        let trout = BitchxCommand::Me {
            text: s("slaps armin around a bit with a large trout"),
        };
        assert_eq!(ok("mircslap armin"), trout);
        assert_eq!(ok("mircslaps armin"), trout);
        assert_eq!(ok("leave #a:b"), BitchxCommand::Part { room: Some(s("#a:b")), reason: None });
        assert_eq!(ok("leave #a:b bye"), BitchxCommand::Part {
            room: Some(s("#a:b")),
            reason: Some(s("bye"))
        });
    }

    #[test]
    fn test_bitchx_quit_exit() {
        assert_eq!(ok("quit"), BitchxCommand::Quit { reason: None });
        assert_eq!(ok("exit"), BitchxCommand::Quit { reason: None });
        assert_eq!(ok("quit see you  later"), BitchxCommand::Quit {
            reason: Some(s("see you  later")),
        });
        assert_eq!(ok("EXIT bye"), BitchxCommand::Quit { reason: Some(s("bye")) });
    }

    #[test]
    fn test_bitchx_nick_topic() {
        assert_eq!(ok("nick Bob The Builder"), BitchxCommand::Nick { nick: s("Bob The Builder") });
        assert_eq!(parse("nick"), Err(missing("nick", "nick")));
        assert_eq!(ok("topic Rust  talk | no spam"), BitchxCommand::Topic {
            topic: s("Rust  talk | no spam"),
        });
        assert_eq!(parse("topic"), Err(missing("topic", "topic")));
    }

    #[test]
    fn test_bitchx_away_back() {
        assert_eq!(ok("away"), BitchxCommand::Away { reason: None });
        assert_eq!(ok("away at lunch"), BitchxCommand::Away { reason: Some(s("at lunch")) });
        assert_eq!(ok("back"), BitchxCommand::Back);
        assert_eq!(parse("back now"), Err(invalid("back", "now")));
    }

    #[test]
    fn test_bitchx_no_arg_commands() {
        let cases = [
            ("clear", BitchxCommand::Clear),
            ("close", BitchxCommand::Close),
            ("channels", BitchxCommand::Channels),
            ("rooms", BitchxCommand::Channels),
            ("next", BitchxCommand::NextWindow),
            ("prev", BitchxCommand::PrevWindow),
            ("previous", BitchxCommand::PrevWindow),
            ("version", BitchxCommand::Version),
            ("help", BitchxCommand::Help),
        ];

        for (input, expect) in cases {
            assert_eq!(ok(input), expect, "{input}");
            assert_eq!(ok(&format!("  {input}  ")), expect, "{input}");

            let err = parse(&format!("{input} junk")).unwrap_err();
            assert!(
                matches!(err, BitchxError::InvalidArgument { ref argument, .. } if argument == "junk"),
                "{input}: {err:?}"
            );
        }
    }

    #[test]
    fn test_bitchx_list_and_channels() {
        let l = |server: Option<&str>, search: Option<&str>| {
            BitchxCommand::List(ListArgs {
                server: server.map(Into::into),
                search: search.map(Into::into),
            })
        };
        assert_eq!(ok("list"), l(None, None));
        assert_eq!(ok("LIST matrix.org"), l(Some("matrix.org"), None));
        assert_eq!(ok("list -s rust"), l(None, Some("rust")));
        assert_eq!(ok("list matrix.org -s rust lang"), l(Some("matrix.org"), Some("rust lang")));
        assert_eq!(parse("list -s"), Err(invalid("list", "-s")));
        assert_eq!(parse("list a b"), Err(invalid("list", "a b")));
        // /rooms is a hidden alias: works, but /help never says room.
        assert_eq!(ok("rooms"), BitchxCommand::Channels);
        assert!(COMMAND_NAMES.contains(&"channels"));
        assert!(!COMMAND_NAMES.iter().any(|n| n.contains("room")));
    }

    #[test]
    fn test_bitchx_who_names() {
        assert_eq!(ok("who"), BitchxCommand::Who { target: None });
        assert_eq!(ok("who #a:b"), BitchxCommand::Who { target: Some(s("#a:b")) });
        assert_eq!(ok("names"), BitchxCommand::Names { target: None });
        assert_eq!(ok("names #a:b"), BitchxCommand::Names { target: Some(s("#a:b")) });
        assert_eq!(parse("who a b"), Err(invalid("who", "b")));
        assert_eq!(parse("names a b"), Err(invalid("names", "b")));
    }

    #[test]
    fn test_bitchx_mode_kick() {
        assert_eq!(ok("mode #a:b"), BitchxCommand::Mode { target: s("#a:b"), modes: None });
        assert_eq!(ok("mode #a:b +o bob"), BitchxCommand::Mode {
            target: s("#a:b"),
            modes: Some(s("+o bob")),
        });
        assert_eq!(parse("mode"), Err(missing("mode", "target")));
        assert_eq!(ok("kick @bob:x"), BitchxCommand::Kick { target: s("@bob:x"), reason: None });
        assert_eq!(ok("kick @bob:x be  nice"), BitchxCommand::Kick {
            target: s("@bob:x"),
            reason: Some(s("be  nice")),
        });
        assert_eq!(parse("kick"), Err(missing("kick", "target")));
    }

    #[test]
    fn test_bitchx_op_deop_query() {
        assert_eq!(ok("op @bob:x"), BitchxCommand::Op { target: s("@bob:x") });
        assert_eq!(ok("deop @bob:x"), BitchxCommand::Deop { target: s("@bob:x") });
        assert_eq!(ok("query @bob:x"), BitchxCommand::Query { target: Some(s("@bob:x")) });
        assert_eq!(parse("op"), Err(missing("op", "target")));
        assert_eq!(parse("deop"), Err(missing("deop", "target")));
        assert_eq!(ok("query"), BitchxCommand::Query { target: None });
        assert_eq!(parse("op a b"), Err(invalid("op", "b")));
        assert_eq!(parse("deop a b"), Err(invalid("deop", "b")));
        assert_eq!(parse("query a hi there"), Err(invalid("query", "hi there")));
    }

    #[test]
    fn test_bitchx_invite() {
        assert_eq!(ok("invite @bob:x"), BitchxCommand::Invite { target: s("@bob:x"), room: None });
        assert_eq!(ok("invite @bob:x #a:b"), BitchxCommand::Invite {
            target: s("@bob:x"),
            room: Some(s("#a:b")),
        });
        assert_eq!(parse("invite"), Err(missing("invite", "target")));
        assert_eq!(parse("invite a b c"), Err(invalid("invite", "c")));
    }

    #[test]
    fn test_bitchx_ctcp() {
        assert_eq!(ok("ctcp bob VERSION  please"), BitchxCommand::Ctcp {
            target: s("bob"),
            text: s("VERSION  please"),
        });
        assert_eq!(parse("ctcp"), Err(missing("ctcp", "target")));
        assert_eq!(parse("ctcp bob"), Err(missing("ctcp", "text")));
    }

    #[test]
    fn test_bitchx_ignore_unignore() {
        assert_eq!(ok("ignore @spam:x"), BitchxCommand::Ignore { pattern: s("@spam:x") });
        assert_eq!(ok("unignore @spam:x"), BitchxCommand::Unignore { pattern: s("@spam:x") });
        assert_eq!(parse("ignore"), Err(missing("ignore", "pattern")));
        assert_eq!(parse("unignore"), Err(missing("unignore", "pattern")));
        assert_eq!(parse("ignore a b"), Err(invalid("ignore", "b")));
    }

    #[test]
    fn test_bitchx_window() {
        assert_eq!(ok("window"), BitchxCommand::Window { target: None });
        assert_eq!(ok("window 3"), BitchxCommand::Window { target: Some(s("3")) });
        assert_eq!(ok("window #a:b"), BitchxCommand::Window { target: Some(s("#a:b")) });
        assert_eq!(ok("window next"), BitchxCommand::NextWindow);
        assert_eq!(ok("window split on"), BitchxCommand::WindowSplit(Some(true)));
        assert_eq!(ok("window SPLIT off"), BitchxCommand::WindowSplit(Some(false)));
        assert_eq!(ok("window split toggle"), BitchxCommand::WindowSplit(None));
        assert!(parse("window split").is_err());
        assert!(parse("window split sideways").is_err());
        assert_eq!(ok("window prev"), BitchxCommand::PrevWindow);
        assert_eq!(ok("window previous"), BitchxCommand::PrevWindow);
        assert_eq!(parse("window 1 2"), Err(invalid("window", "2")));
    }

    #[test]
    fn test_bitchx_every_listed_name_parses() {
        // Every advertised command word is recognised (never UnknownCommand).
        for name in COMMAND_NAMES {
            if let Err(BitchxError::UnknownCommand(c)) = parse(name) {
                panic!("{name} reported unknown ({c})");
            }
        }
    }

    #[test]
    fn test_bitchx_set_parse() {
        assert_eq!(ok("set"), BitchxCommand::Set { name: None, value: None });
        assert_eq!(ok("SET timestamp"), BitchxCommand::Set {
            name: Some(s("timestamp")),
            value: None
        });
        assert_eq!(ok("set timestamp on"), BitchxCommand::Set {
            name: Some(s("timestamp")),
            value: Some(s("on"))
        });
        // The value keeps inner and trailing spaces (`"%I:%M%p "`).
        assert_eq!(ok("set timestamp_string [%H:%M] "), BitchxCommand::Set {
            name: Some(s("timestamp_string")),
            value: Some(s("[%H:%M] "))
        });
    }

    #[test]
    fn test_bitchx_error_display() {
        assert_eq!(BitchxError::UnknownCommand(s("foo")).to_string(), "Unknown command: FOO");
        assert_eq!(missing("msg", "text").to_string(), "/msg: missing argument <text>");
        assert_eq!(invalid("back", "now").to_string(), "/back: invalid argument \"now\"");
    }

    #[test]
    fn test_bitchx_encrypt_trust_devices() {
        assert_eq!(ok("encrypt"), BitchxCommand::Encrypt);
        assert_eq!(ok("ENCRYPT"), BitchxCommand::Encrypt);
        assert_eq!(parse("encrypt now"), Err(invalid("encrypt", "now")));
        assert_eq!(ok("trust"), BitchxCommand::Trust);
        assert_eq!(parse("trust x"), Err(invalid("trust", "x")));
        assert_eq!(ok("devices"), BitchxCommand::Devices { user: None });
        assert_eq!(ok("devices @bob:x"), BitchxCommand::Devices { user: Some(s("@bob:x")) });
        assert_eq!(parse("devices a b"), Err(invalid("devices", "b")));
    }

    #[test]
    fn test_bitchx_verify_subcommands() {
        let v = |c| BitchxCommand::Verify(c);
        assert_eq!(ok("verify"), v(VerifyCmd::List));
        assert_eq!(ok("verify list"), v(VerifyCmd::List));
        assert_eq!(
            ok("verify accept armin"),
            v(VerifyCmd::Accept { user: s("armin"), flow: None })
        );
        assert_eq!(
            ok("VERIFY Accept armin"),
            v(VerifyCmd::Accept { user: s("armin"), flow: None })
        );
        assert_eq!(ok("verify start @a:x"), v(VerifyCmd::Start { user: s("@a:x"), device: None }));
        assert_eq!(
            ok("verify start @a:x ABCDEF"),
            v(VerifyCmd::Start { user: s("@a:x"), device: Some(s("ABCDEF")) })
        );
        assert_eq!(
            ok("verify confirm armin"),
            v(VerifyCmd::Confirm { user: s("armin"), flow: None })
        );
        assert_eq!(
            ok("verify mismatch armin"),
            v(VerifyCmd::Mismatch { user: s("armin"), flow: None })
        );
        assert_eq!(
            ok("verify cancel armin"),
            v(VerifyCmd::Cancel { user: s("armin"), flow: None })
        );
        assert_eq!(
            ok("verify confirm armin f2"),
            v(VerifyCmd::Confirm { user: s("armin"), flow: Some(s("f2")) })
        );
        assert_eq!(
            ok("verify cancel armin f2"),
            v(VerifyCmd::Cancel { user: s("armin"), flow: Some(s("f2")) })
        );
        assert_eq!(parse("verify accept"), Err(missing("verify", "user")));
        assert_eq!(parse("verify start"), Err(missing("verify", "user")));
        assert_eq!(parse("verify confirm"), Err(missing("verify", "user")));
        assert_eq!(parse("verify mismatch"), Err(missing("verify", "user")));
        assert_eq!(parse("verify cancel"), Err(missing("verify", "user")));
        assert_eq!(parse("verify frob armin"), Err(invalid("verify", "frob")));
        assert_eq!(parse("verify accept a b c"), Err(invalid("verify", "c")));
        assert_eq!(parse("verify start a DEV extra"), Err(invalid("verify", "extra")));
    }

    #[test]
    fn test_bitchx_recover_secret() {
        let key = "EsTc 1234 abcd EFGH";
        assert_eq!(ok(&format!("recover {key}")), BitchxCommand::Recover { key: Secret::new(key) });
        assert_eq!(parse("recover"), Err(missing("recover", "recovery key")));
        assert_eq!(parse("recover   "), Err(missing("recover", "recovery key")));
        // The secret never shows up in Debug output.
        let dbg = format!("{:?}", ok(&format!("recover {key}")));
        assert!(!dbg.contains("EsTc") && dbg.contains("***"), "{dbg}");
    }

    #[test]
    fn test_bitchx_mask_secret() {
        assert_eq!(secret_offset("/recover"), None);
        assert_eq!(secret_offset("/recove x"), None);
        assert_eq!(secret_offset("/msg bob x"), None);
        assert_eq!(mask_secret("/recover"), "/recover");
        assert_eq!(mask_secret("/recover "), "/recover ");
        assert_eq!(mask_secret("/recover Es Tc"), "/recover *****");
        assert_eq!(mask_secret("/RECOVER Es"), "/RECOVER **");
        assert_eq!(mask_secret("/  recover ab"), "/  recover **");
        assert_eq!(mask_secret("/recover\u{3000}ab"), "/recover\u{3000}**");
        assert_eq!(redact_secret("/recover EsTc 1234"), "/recover ********");
        assert_eq!(redact_secret("/msg bob hi"), "/msg bob hi");
        assert_eq!(redact_secret("hello"), "hello");
    }

    #[test]
    fn test_bitchx_unicode_bodies() {
        assert_eq!(ok("me ünïcödé  🎉 ok"), BitchxCommand::Me {
            text: s("ünïcödé  🎉 ok")
        });
        assert_eq!(ok("msg 日本 こんにちは 世界"), BitchxCommand::Msg {
            target: s("日本"),
            text: s("こんにちは 世界"),
        });
    }
}
