# v2 vs original BitchX: polish diff

The baseline is the real BitchX-1.3-git client as recorded in `docs/bitchx-recon.md`
(captures in `/workspace/recon/captures`). Each entry gives what v2 showed, what BitchX
shows, and how it was resolved: **fixed** (plus the test that pins it) or **justified**.

## Status bar

1. **Clock format.** v2 showed `[21:24]` (24h). BitchX shows `[09:24pm]` (`%T`, 12-hour,
   lower-case am/pm, bold white). **Fixed:** `status::clock()`.
   Test: `ui::tests::test_status_clock_is_12h_am_pm`.
2. **Channel field with no channel.** v2 printed `  * type /help for help` in window 1.
   BitchX prints an empty `[]` (capture 02: ` [09:20pm][<Nickname not registered yet>] []`).
   **Fixed.** Test: `ui::tests::test_status_window_layout`.
3. **Channel truncation.** v2 printed the whole room name. BitchX cuts `%C` to 10 columns
   (`#bxrecon-0122` becomes `#bxrecon-0`). **Fixed:** `status::CHANNEL_WIDTH`.
   Test: `ui::tests::test_status_field_order`.
4. **Query field.** v2 had no `%Q`. BitchX shows `  [Query: nick]` after the channel
   (capture 29: cyan brackets, white `Query: `, bold white nick). **Fixed:** a DM window
   (Matrix query) shows `[]  [Query: name]`. Test: `ui::tests::test_status_field_order`.
5. **Leading cell colour.** The leading space of status line 1 is bold white on blue
   (`ESC[1m ESC[37m ESC[44m`). It was plain. **Fixed** (it can't be seen, but the attributes now match).
6. **Lag before the first sample.** v2 showed `[Lag 0]` from startup. BitchX shows
   `[Lag ??]` until the first ping, then pads the value to two columns (`[Lag  0]`).
   **Fixed:** `BxStatus::lag` is now `Option<u64>`. It stays `None` until the first
   `MxEvent::Lag`, which comes from the timed `/sync` round-trip.
   Test: `ui::tests::test_status_line2_lag_and_flags`.
7. **Right side of status line 2.** v2 right-aligned `[Win n]`, which BitchX never shows.
   BitchX right-aligns `[%u:%a:%f:%b:%h]`: `[U:a:S:b:h]` in a channel and `[::::]`
   elsewhere, all cyan on blue (captures 21, 22). **Fixed:** it renders exactly as captured.
   **Justified:** these IRC flags mean nothing in Matrix, so they aren't wired to any state.
   The window number is already in the `[#room]` prompt and the `/window list` output.
   Test: `ui::tests::test_status_line2_lag_and_flags`.

## Message formats

8. **Leading glyph.** v2 opened every event and info line with a red/white `-:-` (`$G` from
   the source tree). In the captured client these lines start with a single space
   (` bxrecon23 [~node@host] has joined #chan`). **Fixed:** `format::g()` is now a single
   dark-grey space. Tests: `format::tests::test_join_line_format`, `test_membership_lines`.
9. **Join line colours.** Nick bold cyan (`1;36`), `[ ]` dark grey, user@host cyan, the
   rest plain. v2 used bright cyan. **Fixed:** Matrix maps the mxid to user@host.
   Test: `format::tests::test_join_line_format` (text + per-span colours) and
   `ui::tests::test_room_window_layout`.
10. **Public message brackets.** In v2 other people's `<`/`>` were cyan and your own nick was bold white.
    In BitchX the others' brackets are bold blue, your own are bold magenta, and the nick is
    always plain (capture 23). **Fixed.** Test: `format::tests::test_public_bracket_colors`.
11. **/me.** v2 rendered `* nick text`. BitchX has no `*`. Your own action is ` nick text`
    with the nick in bold white. Someone else's is ` nick/#chan text` in bold yellow (capture 24).
    **Fixed.** Test: `format::tests::test_public_and_action`.
12. **Part reason.** v2 left out `[reason]` when there was none. BitchX always prints it, as `[]`
    when empty. user@host is plain, not grey (capture 32). **Fixed.**
    Test: `format::tests::test_membership_lines`.
13. **Notice.** v2 showed `-nick:#chan- text`. BitchX shows `-nick(user@host)- text`
    (`- ( ) -` dark grey, nick and host magenta, capture 27). **Fixed:** `Line::Notice` now
    carries the sender mxid. Test: `format::tests::test_notice_and_error`.
14. **Errors.** v2 printed `-:- text` in red and `Unknown command: /frob`. BitchX prints
    ` BitchX: Unknown command: FROB` with "BitchX" in bold and the command upper-cased
    (capture 30). **Fixed** for every client error line. Test:
    `format::tests::test_notice_and_error`, `app::tests::test_unknown_command_is_error_line`.
15. **Kick / nick / topic / mode.** The layout and colours already matched captures 24, 27
    and 30, apart from the glyph (#8). **Fixed** by #8. Test: `format::tests::test_membership_lines`.
    **Justified:** `ServerMode/#chan` (bold red) has no Matrix equivalent, since every Matrix
    state change has a sender, so it isn't rendered.
16. **Startup header.** v2 opened window 1 with the logo straight away. BitchX starts with
    `BitchX - Based on EPIC Software Labs epic ircII (1998).`, `Version (...) -- Date (...).`,
    `Process [pid]` and `Using terminal type [$TERM]` (captures 00/01). **Fixed:** those four
    lines now come first, in default colours (`app::startup_lines`). **Justified:** the
    `Date (yyyymmdd)` field shows the git commit instead (`Commit (sha)`), because no build
    date is embedded and adding one would need build.rs or crate changes.
17. **Logo.** BitchX prints about 20 rows of CP437 ANSI art, which turns into mojibake in a
    UTF-8 terminal (recon 7, "DO NOT"). **Justified:** we keep the ASCII figlet logo in bold
    blue, indented, followed by the `[ iambitchx-ver]` credit row like `[ BitchX-1.3-git]`.
    Block glyphs were left out on purpose, because `ui::assert_no_box_glyphs` bans
    U+2500..U+259F everywhere.
18. **BitchX: lines.** v2 printed `iambitchx x -- BitchX for Matrix. Type /help`. BitchX
    prints ` BitchX: Auto Response is set to - nick`, with "BitchX" in bold. **Fixed:** both
    lines now use that form.
19. **Room list.** v2 printed `Window N: #room [n users]`. BitchX prints a numbered server
    list grouped by network: ` [efnet]`, then `      0) host port`. **Fixed** (recon 8 maps
    this to a homeserver line plus rooms): ` Current homeserver: url`, ` Room list:`,
    ` [server_name]`, then `      N) #room [n users]`, where N is the window number,
    right-aligned so that `10)` lines up with `2)`. Test: `ui::tests::test_status_window_layout`.
20. **Idle prompt.** v2 showed `[status]` in window 1. BitchX shows `[0]` (capture 02).
    **Fixed.** Tests: `app::tests` (prompt) and `ui::tests::test_status_window_layout`.
    The idle screen is otherwise just scrollback plus the 2 status lines, as captured: no
    splash and no placeholder.
21. **--smoke window 1.** The 24-row render cut off the top of the banner. **Fixed:** smoke
    now renders window 1 tall enough to show the whole banner and room list.
22. **/names header.** v2 printed `-:- [Users(#chan:n)]` as plain info. BitchX prints
    ` [Users(#chan:n/m)]`: "Users" bold green, the channel and counts green, punctuation
    dark grey (capture 25). **Fixed:** this is now `Line::Users`. Matrix has no
    invisible/visible split, so n = m = the number of joined members. Test:
    `format::tests::test_names_layout`.
23. **/names grid.** v2 printed `[@nick      ]` cells padded to 10. BitchX prints
    `[@bxrecon23 ] [ bxrecon77 ]`, with no leading glyph, the brackets dark grey, `@`
    bold cyan, op nicks bold white, other nicks bold blue, and one space before `]`.
    **Fixed:** this is now `Line::Names`, 5 cells per row. **Justified:** nicks are
    padded to the longest nick in the list so the columns line up; the capture can't
    show BitchX's own padding, because both of its nicks are 9 characters. `+` (voice,
    power 1..49) is shown as bold green. Tests: `format::tests::test_names_layout`,
    `app::tests::test_local_commands`.
24. **Post-join lines.** v2 only focused the window after `/join`. BitchX prints the NAMES
    header and grid, then ` BitchX: Join to #chan was synched in 0.319 secs!!` (capture 22).
    **Fixed:** the new `MxEvent::Synched { room_id, millis }` carries the time the join
    request took, then the window gets NAMES and the synched line before it is focused.
    **Justified:** ` Channel #chan was created at <date>` is left out, because it would
    need an extra state fetch for the m.room.create event on every join. Test:
    `app::tests::test_join_synched`.
25. **/whois layout.** v2 printed `name [mxid]`, ` server : x` and `End of /WHOIS`.
    BitchX prints a green-to-grey ruler `--------------- --  -`, then
    `| nick (user@host) (unknown)`, then rows whose leads alternate ` ` and `| ` and end
    with `: ` on the last row. The leads fade from bold green to green to dark grey, the
    keys are padded to 8 with their first letter bold white, and there is no "End of"
    line (capture 26). **Fixed:** this is now `Line::WhoisRule`, `WhoisNick` and
    `WhoisRow`. The Matrix mapping (recon 8) is: user@host becomes the mxid, `(unknown)`
    becomes the cross-signing state (verified, unverified or unknown), ircname is the
    displayname, channels are the rooms in common (`@` for PL>=50), server is the
    homeserver, and `devices` (the device count) replaces `idle`/`actually`, which
    Matrix doesn't expose. Test: `format::tests::test_whois_layout`.
