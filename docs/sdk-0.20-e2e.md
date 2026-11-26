# matrix-sdk 0.20 live e2e checklist

- binary: target/debug/iambitchx built 11:37Z from 7022914 (matrix-sdk 0.20.0, libsqlite3-sys 0.39.1)
- run: tmux koto-shell.0 via /workspace/bin/iambitchx; restarted 11:38:00Z, again 11:41Z (E2EE restart check)
- log: /workspace/.cache/iamb/logs/iambitchx-log-carla.2026-11-26, checked from L21349 (2791 lines)
- test room: !FiEDqrAfKVSMdkIups:home-srv.test "#sdk020-e2e-test", private_chat preset (invite-only, +i)

## results

- session restore without re-login: PASS. No login prompt; status shows "Connected to https://matrix.home-srv.test/ as @carla:home-srv.test / Device m9LxhmJABj"; 127 restore_session log lines.
- sync + backlog #welcome (read only): PASS. /join #welcome showed backlog from 09:34 onward plus "[Users(#welcome:14/14)]" and "Join to #welcome was synched in 0.255 secs!!". Nothing was posted. 1455 sync log lines.
- /list: PASS. "#home-srv-test:home-srv.test 17 [space] ...", "#welcome:home-srv.test 14 Welcome", "End of /LIST (2 channels)".
- test channel private invite-only, /join: PASS. The client has no room-create command, and /query to self returns 403 "already in the room". The room was created through the client-server API (createRoom, preset private_chat; script reads the token from session.json and never prints it). After that, /join in the client showed "has joined #sdk020-e2e-test", "mode [+o carla]", and the status bar showed "[#sdk020-e2(+i)]".
- send: PASS. "<carla> sdk020 e2e plain message 1"
- /me: PASS. "carla tests emote on sdk 0.20"
- edit: SKIP. iambitchx has no edit command (COMMAND_NAMES in src/bitchx/commands.rs). Not a regression.
- reaction: SKIP. No reaction command (same list).
- redact: SKIP. No redact command (same list).
- E2EE enable + send: PASS. /encrypt + y printed "End-to-end encryption enabled (megolm), +e" and "carla enabled end-to-end encryption"; the status bar showed "(+ie)"; "<carla> sdk020 e2ee encrypted message 1" was sent.
- E2EE decrypt after restart: PASS. After /quit and a restart, rejoining the room showed "<carla> sdk020 e2ee encrypted message 1" in plaintext; 5 m.room.encrypted log lines.
- /whois: PASS. "carla (@carla:home-srv.test) (verified)", "channels : #welcome @#elch @#Armin @#Empty_Room @#sdk020-e2e-test", "devices : 1".
- /names: PASS. "[Users(#sdk020-e2e-test:1/1)] [@carla ]"
- /away + away nick: PASS. "You have been marked as being away (e2e test away)", "carla changed display name to carla afk"; /whois showed "away : carla - e2e test away"; status "(zZzZ 0)".
- /back + display name restored: PASS. "You are no longer marked as being away", "carla changed display name to carla".
- mention highlight: SKIP (explained, not a FAIL). The only possible sender is carla itself, and by design own lines are never highlighted (BitchX semantics, src/bitchx/mention.rs matches own:false only). "carla: mention highlight test" rendered as a normal own line. Highlighting is covered by the mention.rs unit tests (testall green).
- /set timestamp: PASS. "timestamp ON"
- proactive token refresh lines: PASS. "token_refresh: next proactive refresh in 95s (ttl 300s, margin 60s)" and "token_refresh: proactive refresh done" at 11:39:38Z; the timer was rescheduled after the restart.
- 0 M_UNKNOWN_TOKEN since restart: PASS. grep -c M_UNKNOWN_TOKEN = 0
- 0 token strings in log: PASS. grep -cE 'refresh_token [0-9a-f]{16}|mat_[A-Za-z0-9]{10}|mar_[A-Za-z0-9]{10}' = 0
- /part test channel: PASS. /part closed the window; afterwards /whois listed "channels : @#elch @#Armin #welcome @#Empty_Room" (test room gone).
- SAS verification: SKIP (needs a second device/operator; covered by unit tests)
- device-link login: SKIP (needs operator; covered by unit tests)

## side effects

- #welcome: carla joined (read only; it had left at 11:03). /away and /back display-name changes are visible as membership events in shared rooms. This is how the feature is designed.
- test room left; it has no other members.
