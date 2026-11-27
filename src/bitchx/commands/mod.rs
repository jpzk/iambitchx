//! # BitchX slash-command parser
//!
//! Pure parsing of the text typed after a leading `/` in the message bar.
//! Nothing here knows about Matrix; dispatch happens elsewhere.
use std::fmt;

use crate::bitchx::channels::{ListArgs, parse_list_args};

mod secret;

pub(crate) use self::secret::{Secret, mask_secret, redact_secret};

/// A parsed BitchX/IRC-style slash command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BitchxCommand {
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
    /// `/theme`: open the theme picker; `/theme name`: switch to a theme.
    Theme {
        name: Option<String>,
    },
}

/// `/verify` subcommands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VerifyCmd {
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

/// Why a slash command could not be parsed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BitchxError {
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
pub(crate) const COMMAND_NAMES: &[&str] = &[
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
    "theme",
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
pub(crate) fn slap_text(nick: &str) -> String {
    format!("slaps {nick} around a bit with a substantially large Brown Cow...")
}

/// The classic mIRC slap.
pub(crate) fn mircslap_text(nick: &str) -> String {
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
        "accept" => VerifyCmd::Accept {
            user: args.required_word("user")?,
            flow: args.word(),
        },
        "start" => {
            let user = args.required_word("user")?;
            VerifyCmd::Start { user, device: args.word() }
        },
        "confirm" | "yes" => VerifyCmd::Confirm {
            user: args.required_word("user")?,
            flow: args.word(),
        },
        "mismatch" | "no" => VerifyCmd::Mismatch {
            user: args.required_word("user")?,
            flow: args.word(),
        },
        "cancel" => VerifyCmd::Cancel {
            user: args.required_word("user")?,
            flow: args.word(),
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
pub(crate) fn parse(input: &str) -> Result<Option<BitchxCommand>, BitchxError> {
    let Some((word, rest)) = split_word(input) else {
        return Ok(None);
    };

    let lower = word.to_lowercase();
    let canonical: &'static str = match lower.as_str() {
        "exit" => "quit",
        "j" => "join",
        "previous" => "prev",
        "rooms" => "channels",
        other => match COMMAND_NAMES.iter().copied().find(|name| *name == other) {
            Some(name) => name,
            None => return Err(BitchxError::UnknownCommand(word.to_string())),
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
        "theme" => {
            let name = args.word();
            args.finish()?;
            BitchxCommand::Theme { name }
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
mod tests;
