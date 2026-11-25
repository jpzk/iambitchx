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
