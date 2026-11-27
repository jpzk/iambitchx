//! Unit tests for the slash-command parser and secret masking.
use super::secret::secret_offset;
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
    assert_eq!(
        ok("msg bob hello   there  world"),
        BitchxCommand::Msg { target: s("bob"), text: s("hello   there  world") }
    );
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
    assert_eq!(
        ok("join #rust:matrix.org"),
        BitchxCommand::Join { room: s("#rust:matrix.org"), key: None }
    );
    assert_eq!(
        ok("join #a:b secret"),
        BitchxCommand::Join { room: s("#a:b"), key: Some(s("secret")) }
    );
    assert_eq!(ok("j #a:b"), BitchxCommand::Join { room: s("#a:b"), key: None });
    assert_eq!(parse("join"), Err(missing("join", "channel")));
    assert_eq!(parse("join #a:b key extra"), Err(invalid("join", "extra")));
}

#[test]
fn test_bitchx_part_leave() {
    assert_eq!(ok("part"), BitchxCommand::Part { room: None, reason: None });
    assert_eq!(ok("part #a:b"), BitchxCommand::Part { room: Some(s("#a:b")), reason: None });
    assert_eq!(
        ok("part !id:b gone  fishing"),
        BitchxCommand::Part {
            room: Some(s("!id:b")),
            reason: Some(s("gone  fishing")),
        }
    );
    assert_eq!(
        ok("part gone fishing"),
        BitchxCommand::Part { room: None, reason: Some(s("gone fishing")) }
    );
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
    assert_eq!(
        ok("leave #a:b bye"),
        BitchxCommand::Part { room: Some(s("#a:b")), reason: Some(s("bye")) }
    );
}

#[test]
fn test_bitchx_quit_exit() {
    assert_eq!(ok("quit"), BitchxCommand::Quit { reason: None });
    assert_eq!(ok("exit"), BitchxCommand::Quit { reason: None });
    assert_eq!(
        ok("quit see you  later"),
        BitchxCommand::Quit { reason: Some(s("see you  later")) }
    );
    assert_eq!(ok("EXIT bye"), BitchxCommand::Quit { reason: Some(s("bye")) });
}

#[test]
fn test_bitchx_nick_topic() {
    assert_eq!(ok("nick Bob The Builder"), BitchxCommand::Nick { nick: s("Bob The Builder") });
    assert_eq!(parse("nick"), Err(missing("nick", "nick")));
    assert_eq!(
        ok("topic Rust  talk | no spam"),
        BitchxCommand::Topic { topic: s("Rust  talk | no spam") }
    );
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
    assert_eq!(
        ok("mode #a:b +o bob"),
        BitchxCommand::Mode { target: s("#a:b"), modes: Some(s("+o bob")) }
    );
    assert_eq!(parse("mode"), Err(missing("mode", "target")));
    assert_eq!(ok("kick @bob:x"), BitchxCommand::Kick { target: s("@bob:x"), reason: None });
    assert_eq!(
        ok("kick @bob:x be  nice"),
        BitchxCommand::Kick { target: s("@bob:x"), reason: Some(s("be  nice")) }
    );
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
    assert_eq!(
        ok("invite @bob:x #a:b"),
        BitchxCommand::Invite { target: s("@bob:x"), room: Some(s("#a:b")) }
    );
    assert_eq!(parse("invite"), Err(missing("invite", "target")));
    assert_eq!(parse("invite a b c"), Err(invalid("invite", "c")));
}

#[test]
fn test_bitchx_ctcp() {
    assert_eq!(
        ok("ctcp bob VERSION  please"),
        BitchxCommand::Ctcp { target: s("bob"), text: s("VERSION  please") }
    );
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
    assert_eq!(ok("SET timestamp"), BitchxCommand::Set { name: Some(s("timestamp")), value: None });
    assert_eq!(
        ok("set timestamp on"),
        BitchxCommand::Set { name: Some(s("timestamp")), value: Some(s("on")) }
    );
    // The value keeps inner and trailing spaces (`"%I:%M%p "`).
    assert_eq!(
        ok("set timestamp_string [%H:%M] "),
        BitchxCommand::Set {
            name: Some(s("timestamp_string")),
            value: Some(s("[%H:%M] "))
        }
    );
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
    assert_eq!(ok("verify accept armin"), v(VerifyCmd::Accept { user: s("armin"), flow: None }));
    assert_eq!(ok("VERIFY Accept armin"), v(VerifyCmd::Accept { user: s("armin"), flow: None }));
    assert_eq!(ok("verify start @a:x"), v(VerifyCmd::Start { user: s("@a:x"), device: None }));
    assert_eq!(
        ok("verify start @a:x ABCDEF"),
        v(VerifyCmd::Start { user: s("@a:x"), device: Some(s("ABCDEF")) })
    );
    assert_eq!(ok("verify confirm armin"), v(VerifyCmd::Confirm { user: s("armin"), flow: None }));
    assert_eq!(
        ok("verify mismatch armin"),
        v(VerifyCmd::Mismatch { user: s("armin"), flow: None })
    );
    assert_eq!(ok("verify cancel armin"), v(VerifyCmd::Cancel { user: s("armin"), flow: None }));
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
    assert_eq!(ok("me ünïcödé  🎉 ok"), BitchxCommand::Me { text: s("ünïcödé  🎉 ok") });
    assert_eq!(
        ok("msg 日本 こんにちは 世界"),
        BitchxCommand::Msg {
            target: s("日本"), text: s("こんにちは 世界")
        }
    );
}

#[test]
fn test_bitchx_theme_parse() {
    assert_eq!(ok("theme"), BitchxCommand::Theme { name: None });
    assert_eq!(ok("THEME gruvbox-dark"), BitchxCommand::Theme { name: Some(s("gruvbox-dark")) });
    assert_eq!(parse("theme a b"), Err(invalid("theme", "b")));
    assert!(COMMAND_NAMES.contains(&"theme"));
}
