```
    _               ____  _ __       __   _  __
   (_)___ _____ ___/ __ )(_) /______/ /_ | |/ /
  / / __ `/ __ `__ \ __  / / __/ ___/ __ \|   /
 / / /_/ / / / / / / /_/ / / /_/ /__/ / / /   |
/_/\__,_/_/ /_/ /_/_____/_/\__/\___/_/ /_/_/|_|
```

**iamBitchX** is a BitchX-style Matrix client for the terminal.
Channels, not rooms. `/join #channel`, `/msg`, `/whois`, `/away`, `/set` —
like back in the day, with end-to-end encryption underneath.

Project page: <https://jpzk.github.io/iambitchx/> ·
Working notes: [docs/project.md](docs/project.md) ·
Changelog: [CHANGELOG.md](CHANGELOG.md)

Under the hood, iamBitchX runs on the Matrix backend of
[iamb](https://github.com/ulyssa/iamb) by Ulyssa — [iamb.chat](https://iamb.chat)
is the project home (sync, E2EE, store, on top of
[matrix-rust-sdk](https://github.com/matrix-org/matrix-rust-sdk)). The
frontend is new (the Vim UI is gone, BitchX took its place), and login and
token handling were reworked (OAuth, device link, proactive refresh).

## Why BitchX?

Because IRC has been running since 1988 and nobody owns it. No algorithms.
No ads. No age verification. No terms of service. Just text, moving at the
speed of thought, in a terminal that looks like the future imagined by the past.

BitchX was the client that made IRC feel dangerous and exciting. This fork
keeps that energy alive — and gives it a brain.

## History

BitchX was created by panasync (Colten Edwards) in 1995 as a script pack for
ircII, evolving into the most popular terminal IRC client of its era. It was
abandoned around 2004. This fork picks up where it left off, with respect for
the original work and an eye toward the future.

## Build & run

```sh
cargo build --release
./target/release/iambitchx            # default profile
./target/release/iambitchx -P <name>  # another profile
```

On first start you log in with your Matrix account (OAuth or device link).
Type `/help` inside the client.

## Commands

`/join` `/part` `/list` `/names` `/who` `/whois` `/msg` `/query` `/me`
`/topic` `/invite` `/kick` `/op` `/deop` `/mode` `/away` `/back` `/nick`
`/set` `/window` `/next` `/prev` `/clear` `/ignore` `/unignore` `/encrypt` `/verify`
`/trust` `/devices` `/recover` `/theme` `/version` `/quit` `/create`
`/alias` `/publish` `/ban` `/unban` `/kickban` `/power` `/lastlog` `/get` `/upload`

Channel admin (Matrix state behind IRC words):

- `/create #x [-public] [-noencrypt]` — new room `#x` on your own server.
  Default: invite-only (`private_chat`) and end-to-end encrypted. `-public`
  uses `public_chat` and lists it in the room directory; it stays encrypted,
  so late joiners can't read history from before they joined.
- `/join #x:your.server` that does not exist offers to create it (`[y/n]`).
  Other servers keep the "no such channel" error.
- `/alias add|del #x[:server]`, `/publish on|off` for the current room.
- `/ban NICK [reason]`, `/unban NICK`, `/kickban NICK [reason]` (a Matrix
  ban also removes the user).
- `/power NICK 0..100` — raw power level, refused above your own.
- `/mode` — `+i/-i` join rule invite/public, `+m/-m` moderated
  (`events_default` 50/0), `+t/-t` topic lock (`m.room.topic` 50/0),
  `+o/-o NICK` op/deop, `+b/-b NICK` ban/unban. Bare `/mode` prints e.g.
  `+it`; the status bar shows it too.
- Posting in a channel where your power is too low prints
  `read-only channel (power N needed, you have M)`.

Ignore (the Matrix account ignore list, `m.ignored_user_list`, shared with
your other clients):

- `/ignore NICK` or `/ignore @user:server` — ignore a user. A bare nick is
  looked up in the current window's members first, then in all windows;
  ambiguous nicks are refused.
- `/unignore NICK` — stop ignoring (works for ignored users in no window).
- `/ignore` — list the ignored users.
- Messages from ignored users are also hidden client-side, in case the
  server still delivers them; joins/parts stay visible.

Lastlog and logging:

- `/lastlog [-i] TEXT` — print the lines of the current window that contain
  `TEXT` (case-sensitive; `-i` ignores case), each with the time it
  happened, between `Lastlog:` and `End of Lastlog`. `/lastlog -clear`
  removes that output again.
- `/set LOG ON` — irssi-style logs (default `OFF`): every window appends
  plain `HH:MM` stamped lines to `LOG_DIR/<server>/<window>.log`, e.g.
  `logs/example.org/#chan.log`; DMs use the peer's nick, the status window
  `status.log`. Names are sanitised (lowercase, only `[a-z0-9#&+-_.@!=,]`).
  `/set LOG_DIR path` (absolute; default `$XDG_DATA_HOME/iambitchx/logs`).
  Debug output and `/lastlog` views are never logged; files are `0600`,
  directories `0700`, symlinked log files are refused.

Highlights: a line that names your nick, mxid, display name or a
`/set HILIGHT word,word` word (whole words, any case) is shown in the BitchX
hilight colour, its window turns hot in the status bar (`[Act: 3,4*]`) and
the terminal beeps unless `/set BEEP OFF`. `/set NOTIFY ON` (default OFF)
also sends a desktop notification as an OSC 777 / OSC 9 terminal escape
(sanitised; in tmux it needs `set -g allow-passthrough on`).

Media (DCC-ish):

- Media messages are numbered per window: `<nick> [m.image #3] cat.png`.
- `/get [N]` saves the Nth-last media of the window (default: the last);
  `/get #N` the one shown as `#N`; `-o` also opens it (not for types that
  run code: scripts, launchers, HTML, SVG, no extension). Files go to
  `$XDG_DOWNLOAD_DIR` or `~/Downloads/iambitchx/`, the name cut to its last
  path component and cleaned, never overwriting (`cat-1.png`, `cat-2.png`).
  Encrypted attachments are decrypted by matrix-sdk.
- `/upload PATH [caption]` sends a file (`"my file.png"` for spaces);
  `image/*` goes as `m.image`, the rest as file/audio/video. Directories,
  missing files and files over 100 MB are refused. Encrypted rooms get an
  encrypted attachment.

Themes: `/theme` opens a live-preview picker (Up/Down preview, Enter keeps
and saves to `[settings.bitchx] theme`, Esc cancels) over `bitchx` (default),
`terminal` (your terminal's own colours) and 62 bundled iamb themes; drop more
`*.toml` into `~/.config/iambitchx/themes/`. See `docs/iambitchx.5` (THEMES).

More: [docs/bitchx-frontend.md](docs/bitchx-frontend.md),
[docs/bitchx-formats.md](docs/bitchx-formats.md).

## Security

Found a vulnerability? Please report it privately, see [SECURITY.md](SECURITY.md).

## Credits & license

- **Shoutout to [iamb.chat](https://iamb.chat) and to Ulyssa.** iamBitchX is a
  fork of [iamb](https://github.com/ulyssa/iamb) — iamb's Matrix engine, its
  sync loop, its E2EE stack and its store are what make this client possible.
  We swapped the Vim frontend for a BitchX one and reworked login and token
  handling; the hard parts are all still Ulyssa's. Go support the project.
- The "Why BitchX?" and "History" texts are taken from
  [prime001/BitchX](https://github.com/prime001/BitchX).
- iamBitchX is a fork of [iamb](https://github.com/ulyssa/iamb) by Ulyssa,
  used under the Apache License 2.0. See [NOTICE](NOTICE) and [LICENSE](LICENSE).
- BitchX is by panasync (Colten Edwards) and contributors. iamBitchX shares no
  code with it, only the look and the attitude.
