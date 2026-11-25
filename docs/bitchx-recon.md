# BitchX UX recon (for the iambitchx Matrix clone)

- Upstream: https://github.com/BitchX/BitchX1.3
- **Built commit: `b081e19913f2236c19d0cd710af103b38c0cf704`** (2026-02-28, "Merge pull request #11")
- `bx-install/bin/BitchX -v` → `BitchX-1.3-git (20210425)`
- Build: `autoconf && autoheader`, `CFLAGS="-std=gnu89 -fcommon -O2 -w" ./configure --prefix=/workspace/recon/bx-install --with-ssl`, `make && make install` (help files copied by hand because cpio was missing). Nothing was installed outside the prefix.
- Run with: `env HOME=/workspace/recon/bxhome TERM=xterm-256color BitchX -N -n <nick>` (`-N` = don't connect, i.e. offline; `-d` is dumb-terminal mode, not offline). This ran in tmux session `bxrecon` at 100x30.
- Captures: `captures/NN-name.ansi` (from `tmux capture-pane -e -p`) plus `.txt`. Also `captures/keybindings.txt` (`/bind`) and `captures/status-formats.txt` (`/set status*`).
- Online test: **irc.libera.chat:6697 TLS connected, but the server dropped the link.** Libera requires SASL from this network: "SASL authentication to a NickServ account with a verified email address is required to connect from your current network" (capture 20). Port 6667 is the same network and has the same policy. The goal forbids registering nicks, so Libera was not usable. BitchX then fell back on its own to the next server in its built-in list, **EFnet irc.eversible.com:6667** (ratbox, no auth). All online captures (21–32) come from EFnet. Nicks were `bxrecon23` and `bxrecon77`/`bxrecon78` (EFnet NICKLEN=9 truncated `bxrecon2317`). Channel `#bxrecon-0122` was created fresh and only my two connections were in it. Online time was about 2.5 minutes (21:23:31 to 21:25:41 UTC). I messaged nobody else and registered nothing.

Color notation below: SGR codes as captured. `1;30` = bold black, which shows as **dark grey** and is BitchX's signature punctuation color. `36` = cyan, `44` = blue background.

---

## 1. Layout

Single window, 100x30 terminal (captures 02, 22):

```
row 1..27  output / scrollback (window 1)
row 28     status line 1   (blue bg)
row 29     status line 2   (blue bg)
row 30     input line, prompt = current target in [..]
```

- With `/window split on`, row 1 becomes a **top status bar** (`STATUS_FORMAT3` = `BitchX by panasync`, bold white on blue) and output starts at row 2 (capture 19).
- `/window new` splits the screen horizontally. Each window has its own output area and its own status bar right below it. The new window gets a **1-line** status bar, while window 1 keeps its 2-line one (capture 07). The input line is shared at the very bottom.
- Lines wrap with a hanging indent. Continuation lines line up under the text after the nick or prefix (see the server 005 lines and the libera notice in capture 20).
- No nicklist pane, no channel list pane, no tabs. Everything is text in the window plus the status bars.

## 2. Status bars

Formats (from `/set`, capture 18 and `status-formats.txt`):

- `STATUS_FORMAT1  " [%T][%*%@%=%N%#%A]%M [%C%+%W] %Q %H%B%F"`
- `STATUS_FORMAT2  "%L %! %K %>%D %J[%u:%a:%f:%b:%h]"`
- `STATUS_FORMAT3  "BitchX by panasync"` (top bar, only shown when split)

Rendered in a channel (capture 22):

```
 [09:24pm][@bxrecon23(+iw)] [#bxrecon-0(+nt)]  [Query: bxrecon78]
 [Lag ??] [O/1 N/1 I/0 V/0 F/0]                       [U:a:S:b:h]
```

Fields, line 1:
- whole bar: blue background (`44`), default fg white (`37`)
- `[` `]` `(` `)` `+` punctuation: cyan (`36`)
- `%T` clock `09:24pm`: **bold white**. 12-hour format with am/pm. Shown only on the *current* window.
- `%@` op marker `@`: bold white. `%N` nick: white (non-bold).
- `%#` user modes `+iw`: `+` cyan, letters white. The parens around them are bold.
- `%C` channel: white, **truncated to 10 chars** (`#bxrecon-0`). `%+` channel modes `(+nt)` styled like user modes.
- `%Q` query: `[Query: nick]` only while a query is active (capture 29).
- Before registration it reads `[<Nickname not registered yet>] []` (capture 02).

Fields, line 2:
- `[Lag ??]` before the first ping, then `[Lag  0]`. "Lag" is white, the value bold.
- `[O/1 N/1 I/0 V/0 F/0]` = channel counts of Ops / Non-ops / Ircops / Voiced / Friends. Letters white, `/` cyan, numbers bold white. Only shown in a channel.
- Right-aligned `[U:a:S:b:h]` = `%u:%a:%f:%b:%h` flags. Offline it is just `[::::]`.
- With several windows visible, the current one shows `^^^^^^^^` (STATUS_WINDOW, bold yellow) in its bar and the others don't. Other windows also don't show the clock (captures 07, 19).

## 3. Message formats (colors as captured)

Everything is a single line. There is **no timestamp per line** by default; the time is only shown in the status bar.

- **Own channel message:** `<bxrecon23> text`. `<` and `>` are bold magenta (`1;35`), nick default color, text default.
- **Other's channel message:** `<bxrecon77> text`. `<` and `>` are bold blue (`1;34`). So yours is magenta and theirs is blue, and only the brackets are colored.
- **Message to a non-current target:** `<bxrecon23:#nonexistent-chan-0122> x`. Brackets plain magenta, `:` dark grey.
- **Action (/me) own:** ` bxrecon23 waves at itself`. Leading space, nick bold white, no `*`.
- **Action from another nick:** ` bxrecon77/#bxrecon-0122 waves back`. Nick and `/#channel` bold yellow (`1;33`).
- **Private message received:** `[bxrecon78(~node@host)] private reply`. `[ ( ) ]` dark grey, nick and host magenta.
- **Private message sent:** `[msg(bxrecon78)] private hello`. `msg` and nick red (`31`), punctuation dark grey.
- **Query start/end:** ` Starting conversation with bxrecon78` / ` Ending conversation with bxrecon78` (plain).
- **Notice received:** `-bxrecon77(~node@host)- test notice`. `-`, `(`, `)` dark grey, nick and host magenta.
- **Server notice:** `-tantalum.libera.chat(***)- text`.
- **Join:** ` bxrecon23 [~node@host] has joined #chan`. Nick bold cyan, `[ ]` dark grey, user@host cyan, channel plain.
- **Part:** ` bxrecon78 [~node@host] has left #chan []`. `[reason]` is always printed, even when empty.
- **Kick:** ` bxrecon78 was kicked off #chan by bxrecon23 (reason)`. Kicker cyan, `( )` dark grey. BitchX **auto-rejoins on kick** by default; the join line followed immediately.
- **Mode (user):** ` mode/#chan [+o bxrecon78] by bxrecon23`. `/` dark grey, channel cyan, `[ ]` dark grey, mode text bold white, setter bold white.
- **Mode (server):** ` ServerMode/#chan [+nt] by irc.server`. `ServerMode` bold red.
- **Topic change:** ` Topic (#chan): changed by bxrecon23: new topic`. `( ) :` dark grey, channel and nick cyan.
- **Nick change:** ` bxrecon77 is now known as bxrecon78`. Old nick bold white, new nick cyan.
- **Names (on join and /names):** a header line ` [Users(#chan:2/2)]` ("Users" green, numbers green, punctuation dark grey). Then a grid of `[ nick ]` cells (`[ ]` dark grey, `@` cyan, nick bold white), several per row (capture 25).
- **After join:** ` Channel #chan was created at <date>` and ` BitchX: Join to #chan was synched in 0.319 secs!!` ("BitchX" bold).
- **Whois:** box style with a green/grey dashed header `------ ------ --- --  -`. Rows start with `|`, ` `, or `:` (dark grey), field names are padded (`ircname  :`, `channels :`, `server   :`, `idle     :`) and the first letter is bold white (capture 26).
- **Error, numeric:** `#nonexistent-chan-0122: No such nick/channel`. Target bold white, `:` dark grey.
- **Error, client:** ` BitchX: Unknown command: NOSUCHCOMMAND`. "BitchX" bold, and the command is upper-cased.
- **Client info lines:** a leading space, often prefixed `BitchX:`.
- **Connect sequence:** ` Connecting to port 6697 of server X [refnum 51]`, ` SSL server connected`, then server notices. LUSERS is reformatted as ` [local users on irc(732)] 9%` and so on, then ` Mode change [+i] for user nick`.
- **Disconnect:** ` Closing Link: ... (reason)`, ` Connection closed from X: Remote end closed connection`, and BitchX moves straight on to the next server in its list.

## 4. Windows

- Windows are numbered from 1. `/window new` creates the next number. The input prompt shows the target (`[0]` when there is none, otherwise `[#chan]` or `[nick]`).
- `/window list` gives a table of refnum, name, nick, channel, server and visibility (capture 08).
- Switching:
  - `Ctrl-W n` / `Ctrl-W p`: next / previous window
  - `Ctrl-W l`: list, `h`: hide, `k`: kill, `b`: balance, `+` / `-`: grow / shrink
  - **`Alt-1`..`Alt-0` = WINDOW_SWAP_n.** This swaps a *hidden* window into the current slot. If the window is already visible it fails with `SWAP: Window 2 is not hidden!` (captures 09, 13, 15). It is not a plain "go to window n".
  - `/window hide` and `/window show N` (captures 12, 14).
- `/window split on|off|toggle` turns the top bar on and off.
- **Ctrl-X = SWITCH_CHANNELS.** It cycles the current window's target through the channels you have joined (capture 31). One window can hold several channels.
- Only the current window shows the clock and the `^^^^^^^^` marker.

## 5. Input

- Single line at the bottom. The prompt is `[target]`: `[0]` with no target, `[#bxrecon-0122]` in a channel, `[bxrecon78]` during `/query`.
- One input line only (no multi-line composer).
- History: Up/Down and `Ctrl-P` go back. **`Ctrl-N` is bound to QUOTE_CHARACTER**, not "next" (from the `/bind` dump).
- Tab completes **commands** and upper-cases them (`/wind` becomes `/WINDOW`, capture 16). `Ctrl-R` = NICK_COMPLETION (`/bind`). Tab = TAB_COMPLETION.
- `/query nick` sets the default target. `/query` with no argument ends it.
- Unknown commands give ` BitchX: Unknown command: X`. `/command` abbreviations work when they are unambiguous.
- `Ctrl-L` redraws. `Alt-.` clears.

## 6. Scrollback

- `PgUp` / `PgDn` scroll back/forward (capture 17 shows the view scrolled into the /set listing; status bars unchanged — no visible "scrolled" indicator was observed in this capture).
- `Alt-<` / `Alt->` = SCROLL_START / SCROLL_END, `Alt-p` / `Alt-n` = SCROLL_BACKWARD / FORWARD, `Alt-e` = end (from `/bind`).
- `/clear` empties the window.

## 7. Idle / startup

Startup (captures 00, 01):

```
BitchX - Based on EPIC Software Labs epic ircII (1998).
Version (BitchX-1.3-git) -- Date (20210425).
Process [5186]
Using terminal type [vt100]
<~20-line CP437 ANSI-art "BitchX" logo, credits "PhonyEye" / "Awe/Cia",
  "[ BitchX-1.3-git]">
 BitchX: Created directory ~/.BitchX
 BitchX: Auto Response is set to - <nick>
<numbered server list grouped by network: [EFNet] 1) ... [oftc] 46) ...>
```

- In a UTF-8 terminal the logo is CP437 bytes and shows up as mojibake. A clone should ship a UTF-8 or ASCII version of it.
- Idle, offline (capture 02): the screen is just the scrollback, then status 1 ` [09:20pm][<Nickname not registered yet>] []`, status 2 with only `[::::]` on the right, and prompt `[0]`. There is no splash, help hint or placeholder. The clock ticks once a minute.
- Idle, connected: after the MOTD/LUSERS dump, the status shows `[09:24pm][nick(+iw)] []` and `[Lag ??]`.

## 8. Matrix mapping: do / do not

DO
- Keep the exact 3-zone layout: output rows, a **2-line blue status bar** (cyan punctuation, bold white clock and values), and a one-line input with a `[target]` prompt. Add the optional top bar on split.
- Map **channel → room**. Show the room alias or name in `%C`, truncated like BitchX (about 10 chars). Map **query → DM room** and show `[Query: @user]` in status line 1.
- Use BitchX line formats exactly: magenta `<me>` vs blue `<them>`, bold-cyan join nick with grey `[mxid-server]` in place of `[user@host]`, `has left #room [reason]`, `was kicked off ... by ... (reason)`, `mode/#room [+o x] by y` for power-level changes (map PL ≥ 50 to `@`, PL ≥ 1 to `+`).
- Map the `O/N/I/V/F` counters to Matrix concepts: O = PL≥50 (admins/mods), V = PL>0, N = everyone else, I = server admins (or drop it), F = friends/DM'd users.
- Show the topic as ` Topic (#room): changed by x: ...`. Map displayname changes to `x is now known as y`. Map `m.notice` to `-nick(mxid)- text` and `m.emote` to the `/me` format.
- On join, print the `[Users(#room:n/n)]` header and the `[ nick ]` grid, then `Join to #room was synched in X secs!!` (time it from the `/sync` that delivered the room).
- Windows numbered from 1, `Ctrl-W n/p`, `/window new|split|hide|show|list`, `Ctrl-X` to cycle rooms inside one window, PgUp/PgDn plus `Alt-<` / `Alt->` for scrollback.
- Command-style input: `/join #alias:server`, `/part`, `/query @user:server`, `/me`, `/topic`, `/names`, `/whois` (show profile, devices count, rooms in common), `/nick` (set displayname), `/msg`.
- Startup banner: version lines plus a UTF-8 logo, then a homeserver line instead of the server list. Idle prompt `[0]`, and `<Not logged in>` in place of `<Nickname not registered yet>`.
- Print errors as one plain line: ` BitchX: Unknown command: X`, `target: M_FORBIDDEN ...`.

DO NOT
- Don't add a nicklist sidebar, room-list sidebar, tab bar, or mouse UI. BitchX has none of these.
- Don't put a timestamp on every line by default. The clock lives in the status bar. Make per-line timestamps opt-in.
- Don't copy the Alt-digit *swap-hidden* behavior as-is. It confuses users who expect "go to window n". Either keep it with clear docs or make Alt-n "focus window n"; choose deliberately.
- Don't bind Ctrl-N to quote-char unless you want strict fidelity. Ctrl-P/Ctrl-N for history is what users expect.
- Don't auto-rejoin after a kick. That is IRC etiquette baggage; in Matrix a kick is a membership event and rejoining may be blocked.
- Don't auto-fail-over to another homeserver. BitchX silently jumped from Libera to EFnet. With Matrix identity tied to the homeserver, that would be a security footgun.
- Don't render threads, reactions, edits or redactions as new UI widgets. Fold them into lines (`* edited`, `[reaction 👍 by x]`) so everything stays single-line text.
- Don't show raw mxids where BitchX shows a nick. Show the displayname and put the mxid in the grey `[...]` host slot.
- Don't put tokens or passwords in status lines, scrollback or logs. BitchX echoes `/server host:port:password` into the server list, so the clone must never echo the access token.
- Don't print the CP437 logo raw in UTF-8 terminals.

## Capture index
00-startup-scrollback, 01-startup, 02-idle-status-window, 03-help, 04-help-window, 05-set-listing, 06-window-help, 07-window-new-split, 08-window-list, 09-alt1-switch, 10-bind-listing, 11-ctrlw-prev-window, 12-window-hidden, 13-alt2-swap, 14-window-show2, 15-alt1-swap-hidden, 16-tab-completion, 17-scrollback-pgup, 18-set-status-formats, 19-window-split, 20-libera-sasl-reject, 21-connected-efnet, 22-channel-join, 23-channel-messages, 24-me-topic, 25-names, 26-whois, 27-notice-nick, 28-second-client-view, 29-query, 30-mode-kick-errors, 31-ctrlx-switch-channel, 32-part

> Note: not copied into /workspace/iambitchx/docs — bx-v2 had uncommitted work in progress (2026-11-25 21:28 UTC). Copy manually once that repo is clean.
