# UI security and display fixes (BitchX front end)

Findings from `goal/gh-advisories/findings.md` that live in, or can be
mitigated from, the BitchX UI (`src/bitchx/**`). Each entry is either
**fixed** (with the commit) or **not fixable UI-only**, with the exact
backend change that is still needed. No backend file was changed for this
work.

## F28 nick = bare localpart (cross-server impersonation) -- fixed (8fe58dc)

- `model::nick_of` keeps a registry of every user ID seen (members and
  senders). A nick shared by two users (case-insensitive localpart) is
  qualified IRC-style: `bob` (our own homeserver) next to `bob@evil.org`;
  two remote twins are both qualified (`eve@a.org`, `eve@b.org`); a remote
  user with our own localpart is always `me@server`, never `me`.
- Every nick-taking command (`/op /deop /kick /msg /query /whois /invite
  /verify /devices`) resolves through `App::resolve_user`: full mxid,
  qualified `nick@server`, or a shown nick. A nick that fits more than one
  user is refused: `eve: Ambiguous nick, matches @eve:a.org @eve:b.org --
  use the full user ID`. There is no `/ban` command in the UI (Matrix bans
  would go through the same resolver).
- Tests: `app::tests::test_f28_localpart_collision_disambiguated`,
  `test_f28_ambiguous_privileged_commands_refused`.

## F29 room display name spoofing a channel alias -- fixed UI-side (8fe58dc), backend change recommended

- A window whose name clashes with another window, or which poses as an
  alias on our own homeserver (`#x:<our server>`, real ones are shown short
  as `#x`), shows its **room ID** in the prompt, status bar and scrollback.
- `/join #alias` always goes to the server (alias resolution); it no longer
  switches to a window that merely carries that name. `/join !id` of a joined
  room just switches.
- `/msg`, `/query`, `/part`, `/window`, `/invite`, `/names`, `/who` refuse a
  name that fits several windows and list the room IDs; room IDs and window
  numbers are always exact. The "new window" status line shows the room ID.
- Test: `app::tests::test_f29_alias_lookalike_room_name`.
- Still needed in the backend (the UI cannot see the canonical alias):
  `src/matrix.rs` `channel_name()` falls back to the display name and
  prefixes `#`, so a room *named* `#rust:matrix.org` (or just `rust`) is
  indistinguishable from the alias when it is the only such window. Change:
  add `pub canonical_alias: Option<String>` to `model::RoomSnapshot` and fill
  it in `snapshot()` from `room.canonical_alias()`; in `channel_name()` never
  emit a leading `#` for a display-name fallback (e.g. `+Room_name` or the
  room ID when the name starts with `#`/`!`). The UI then marks windows with
  `canonical_alias == None` and matches `#` targets against
  `canonical_alias` only.

## F30 bidi / zero-width / Cf format characters -- fixed (8fe58dc)

- `text::is_format` covers the whole Unicode `Cf` category (bidi
  embeddings/overrides/isolates/marks, ZWSP/ZWNJ/ZWJ, word joiner, BOM,
  soft hyphen, invisible operators, tags, ...). `format::sanitize` (used by
  every rendered span, the prompt and the status bar, and by the backend
  for names and topics) drops them; `App::handle_mx` cleans every incoming
  event (`text::clean_line`, `clean_snapshot`).
- Tests: `text::tests::test_bidi_and_zero_width_stripped`,
  `app::tests::test_f30_f31_remote_text_cannot_forge_status_lines`.

## F31 newline / control chars forging `-:-` status lines -- fixed (8fe58dc)

- Every remote one-line field (nick, mxid, display name, topic, room name,
  part/kick reason, verification cancel reason, SAS names, away messages,
  and whole `Info`/`Error` lines from the Matrix side such as
  `x changed the channel name to ...` or `sync: <server error>`) has its
  controls and U+2028/U+2029 replaced by spaces at ingress, so it renders as
  exactly one line. Message bodies keep their newlines; their continuation
  rows are indented under the text and cannot start where a client line
  starts.
- Tests: `text::tests::test_one_line_has_no_breaks`, `test_clean_line_fields`,
  `app::tests::test_f30_f31_remote_text_cannot_forge_status_lines`.

## F32 percent_decode panic -- fixed (cb5746b)

- `channels::percent_decode` decodes on bytes; `%` followed by a multi-byte
  character no longer slices a `&str` off a char boundary.
- Test: `channels::tests::test_percent_decode_non_char_boundary`.

## F37 sync death -- fixed UI-side (84a3510), backend hook recommended

- `spawn_sync` (`src/matrix.rs`) already tells the UI when the sync loop
  breaks for good: `MxEvent::Info { room_id: None, line: Line::Error("sync:
  <err>") }`. `App` recognises that line (`is_sync_death`), sets
  `App::sync_lost`, and the status bar replaces `[Lag ..]` with a bold red
  `[SYNC LOST]` that stays until a restart (a later lag sample does not
  hide it). The status window and the current window get one -:- line:
  `Sync with the homeserver stopped: no new messages arrive until
  iamBitchX is restarted`.
- Test: `app::tests::test_f37_sync_death_marker`.
- Still needed in the backend: the other break in `spawn_sync`, when
  `login::recover` returns `Err(e)`, sends the bare `{e}` without the
  `sync:` prefix, so it is shown but not detected. Change: replace both
  string signals with a typed `MxEvent::SyncStopped { reason: String }`
  (new variant in `src/bitchx/model.rs`, sent from both `break` arms of
  `spawn_sync`), and a `MxEvent::SyncResumed` if a restart of the loop is
  ever added.

## F43 identity change / unverified devices -- not fixable UI-only

The UI model gets no per-message or per-user trust data: `Stamped` /
`Line::Public|Action|Private` carry no verification state, `Member` has
none, and there is no event for a changed identity. The only trust text is
backend-formatted (`/whois` "unverified" in `src/matrix.rs`, `/devices` in
`src/e2ee.rs`). Backend change needed (no UI change was made):

- `src/bitchx/model.rs`: add `pub sender_trust: Option<Trust>` to `Stamped`
  (`enum Trust { Verified, Unverified, Unsigned }`, `None` = unencrypted).
- `src/matrix.rs` (timeline -> `Stamped`): fill it from
  `event.encryption_info().verification_state` (`Verified` ->
  `Verified`, `Unverified(VerificationLevel::UnsignedDevice)` -> `Unsigned`,
  any other `Unverified(_)` -> `Unverified`).
- `src/matrix.rs` (`spawn_sync`): subscribe to
  `client.encryption().user_identities_stream()` and for every changed
  identity of a user we share a room with, send a new
  `MxEvent::IdentityChanged { user, verified_before: bool }`; and per room
  `room.subscribe_to_identity_status_changes()` for
  `IdentityState::PinViolation | VerificationViolation`.
- UI then (small, src/bitchx only): render a dim `[u]` before the nick when
  `sender_trust != Verified`, add an "unverified devices" row to `/whois`,
  and print `-:- WARNING: <nick> (<mxid>) changed their identity
  (cross-signing key)` in every shared window plus the status window.

## F44 verification keyed by user, not flow -- fixed UI-side (84a3510), backend change recommended

- The pending-verification list in `App` now holds one entry per flow id;
  a second request from the same user (e.g. another of our own devices) no
  longer replaces the first. A `-:-` error says another flow is pending.
- `/verify accept|confirm|mismatch|cancel <user> [flow]`: with more than
  one flow for that user, the command is refused unless a flow id (or a
  unique prefix) is given; the error lists `flow [device]` for each.
  `/verify` lists the flow id of every entry.
- The Matrix side still reports progress per user (`VerifyUpdate` has no
  flow id), so when SAS emoji arrive while several flows with that user are
  pending, every such flow is marked ambiguous and `/verify confirm` refuses
  it ("cancel and start again"): the emoji on screen can never be confirmed
  for a different flow. Our own `/verify cancel <user> <flow>` is matched
  to the next `Cancelled` so the other flow survives.
- Test: `app::tests::test_f44_verification_keyed_by_flow`.
- Still needed in the backend for full precision: in `src/e2ee.rs`
  `update()`, send the flow id with every update (`MxEvent::Verify { user,
  flow_id, update }` in `src/bitchx/model.rs`; the SAS and request
  watchers already hold `req.flow_id()` / `sas.flow_id()`), and pass the
  other device id to the SAS header (`sas.other_device().device_id()`), so
  the UI can attribute emoji to a flow instead of refusing.

## Display: status window `[0]` and empty `[]` -- fixed (8fe58dc)

- The status window has no channel, so the prompt read `[0]` and `[%C]` an
  empty `[]`. As in the real BitchX captures, both now read the window name
  (`[status]`).
- Test: `app::tests::test_status_window_shows_its_name`.

## Display: query status bar without `+e` -- fixed (8fe58dc)

- A query (DM) window has no `[%C]`, so its modes were never shown. The
  `[Query: x]` bracket now carries them: `[Query: x(+e)]` for an encrypted
  DM.
- Test: `app::tests::test_query_status_shows_plus_e`.
