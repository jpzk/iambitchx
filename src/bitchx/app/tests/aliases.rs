//! Tests for `/alias` and `/bind`: expansion, recursion limit, built-ins
//! that cannot be shadowed, key dispatch and persistence.

use super::*;

fn sent(fx: &[Effect]) -> Vec<(String, bool)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Send { text, emote, .. } => Some((text.clone(), *emote)),
            _ => None,
        })
        .collect()
}

/// An app with a scratch `config.toml`, so `/alias` and `/bind` save quietly
/// (without one they report `Not saved`, like `/set`).
fn app_cfg(tag: &str) -> App {
    let dir = std::env::temp_dir().join(format!("bx-alias-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[settings]\n").unwrap();
    let mut app = app();
    app.config_toml = Some(path);
    app
}

#[test]
fn test_alias_expands_args() {
    let mut app = app_cfg("expand");
    app.handle_key(alt('2'));
    typed(&mut app, "/alias greet /me waves at $1 and $2, says $*");
    assert_eq!(last(&app), Line::Info("Alias GREET added: /me waves at $1 and $2, says $*".into()));
    let fx = typed(&mut app, "/GREET bob armin");
    assert_eq!(sent(&fx), vec![("waves at bob and armin, says bob armin".into(), true)]);
    // missing args expand to nothing
    let fx = typed(&mut app, "/greet bob");
    assert_eq!(sent(&fx), vec![("waves at bob and , says bob".into(), true)]);
    // no $ reference: args are appended
    typed(&mut app, "/alias wave me waves");
    let fx = typed(&mut app, "/wave hard");
    assert_eq!(sent(&fx), vec![("waves hard".into(), true)]);
}

#[test]
fn test_alias_chain_and_recursion_limit() {
    let mut app = app();
    app.handle_key(alt('2'));
    // a1 -> a2 -> ... -> a8 -> /me: 8 expansions are allowed
    for i in 1..8 {
        typed(&mut app, &format!("/alias a{i} a{} $*", i + 1));
    }
    typed(&mut app, "/alias a8 me deep $*");
    let fx = typed(&mut app, "/a1 x");
    assert_eq!(sent(&fx), vec![("deep x".into(), true)]);
    // a0 -> a1 ... is one expansion too many
    typed(&mut app, "/alias a0 a1 $*");
    let fx = typed(&mut app, "/a0 x");
    assert!(fx.is_empty());
    assert_eq!(last(&app), Line::Error("Alias recursion too deep (limit 8)".into()));
    // a loop stops at the limit instead of hanging
    typed(&mut app, "/alias ping pong");
    typed(&mut app, "/alias pong ping");
    assert!(typed(&mut app, "/ping").is_empty());
    assert_eq!(last(&app), Line::Error("Alias recursion too deep (limit 8)".into()));
}

#[test]
fn test_alias_cannot_shadow_builtins() {
    let mut app = app();
    app.handle_key(alt('2'));
    for name in ["join", "MSG", "j", "rooms", "bind", "alias"] {
        typed(&mut app, &format!("/alias {name} me pwned"));
        assert!(
            matches!(last(&app), Line::Error(ref e) if e.contains("built-in")),
            "{name}: {:?}",
            last(&app)
        );
    }
    assert!(app.aliases.is_empty());
    // a config.toml alias for a built-in is skipped, with a message
    let cfg = crate::bitchx::vars::BitchxConfig {
        aliases: Some([("join".to_string(), "me pwned".to_string())].into()),
        ..Default::default()
    };
    app.load_aliases_binds(&cfg);
    assert!(app.aliases.is_empty());
    assert!(matches!(last(&app), Line::Error(ref e) if e.contains("built-in")));
    // /join still joins
    assert!(matches!(typed(&mut app, "/join #new:x")[..], [Effect::Join(_)]));
}

#[test]
fn test_alias_list_show_remove() {
    let mut app = app_cfg("list");
    typed(&mut app, "/alias");
    assert_eq!(last(&app), Line::Info("No aliases".into()));
    typed(&mut app, "/alias hi me hi");
    typed(&mut app, "/alias hi");
    assert_eq!(last(&app), Line::Info(format!("{:<12} /me hi", "HI")));
    typed(&mut app, "/alias");
    assert_eq!(last(&app), Line::Info(format!("{:<12} /me hi", "HI")));
    typed(&mut app, "/alias -hi");
    assert_eq!(last(&app), Line::Info("Alias HI removed".into()));
    typed(&mut app, "/alias -hi");
    assert_eq!(last(&app), Line::Error("No such alias HI".into()));
    typed(&mut app, "/hi");
    assert_eq!(last(&app), Line::Error("Unknown command: HI".into()));
}

#[test]
fn test_bind_runs_command() {
    let mut app = app_cfg("bind");
    app.handle_key(alt('2'));
    typed(&mut app, "/bind F2 /me pressed f2");
    assert_eq!(last(&app), Line::Info("F2 bound to /me pressed f2".into()));
    let fx = app.handle_key(key(KeyCode::F(2)));
    assert_eq!(sent(&fx), vec![("pressed f2".into(), true)]);
    // a bind runs aliases, and wins over the default key (^N: next window)
    typed(&mut app, "/alias yo me yo $*");
    typed(&mut app, "/bind ^N yo there");
    let fx = app.handle_key(ctrl('n'));
    assert_eq!(sent(&fx), vec![("yo there".into(), true)]);
    assert_eq!(app.refnum(), 2);
    // meta keys
    typed(&mut app, "/bind M-x /me meta");
    assert_eq!(sent(&app.handle_key(alt('x'))), vec![("meta".into(), true)]);
    typed(&mut app, "/bind");
    assert_eq!(last(&app), Line::Info(format!("{:<6} /yo there", "^N")));
    typed(&mut app, "/bind -^N");
    assert_eq!(last(&app), Line::Info("^N unbound".into()));
    app.handle_key(ctrl('n'));
    assert_eq!(app.refnum(), 3);
    typed(&mut app, "/bind ^I /me tab");
    assert!(matches!(last(&app), Line::Error(_)));
    typed(&mut app, "/bind -F9");
    assert_eq!(last(&app), Line::Error("F9 is not bound".into()));
}

#[test]
fn test_alias_bind_persist() {
    let dir = std::env::temp_dir().join(format!("bx-aliasbind-persist-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[settings]\nlog_level = \"info\"\n").unwrap();
    let mut app = app();
    app.config_toml = Some(path.clone());
    typed(&mut app, "/alias hi me hi $1");
    typed(&mut app, "/bind f5 hi bob");
    typed(&mut app, "/set beep off");
    assert!(!matches!(last(&app), Line::Error(_)), "{:?}", last(&app));

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[settings.bitchx.aliases]"), "{text}");
    assert!(text.contains("[settings.bitchx.binds]"), "{text}");
    let cfg = crate::config::Config::parse_toml(&text).unwrap();
    let settings = cfg.settings.unwrap();
    assert_eq!(settings.log_level.as_deref(), Some("info"));
    let bx = settings.bitchx.unwrap();
    let mut fresh = super::app();
    fresh.load_aliases_binds(&bx);
    assert_eq!(fresh.aliases, app.aliases);
    assert_eq!(fresh.binds, app.binds);
    assert_eq!(bx.binds.unwrap().get("F5").map(String::as_str), Some("/hi bob"));

    typed(&mut app, "/alias -hi");
    typed(&mut app, "/bind -F5");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("aliases") && !text.contains("binds"), "{text}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_alias_without_config_toml_reports_not_saved() {
    let mut app = app();
    typed(&mut app, "/alias hi me hi");
    assert_eq!(last(&app), Line::Error("Not saved: no config.toml (JSON config)".into()));
    // the alias still works for this session
    assert!(app.aliases.get("hi").is_some());
}
