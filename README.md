<div align="center">

[![Build Status](https://github.com/jpzk/iambitchx/actions/workflows/ci.yml/badge.svg)](https://github.com/jpzk/iambitchx/actions?query=workflow%3ACI+)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

</div>

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
Working notes: [docs/project.md](docs/project.md)

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
`/set` `/window` `/next` `/prev` `/clear` `/ignore` `/encrypt` `/verify`
`/trust` `/devices` `/recover` `/version` `/quit`

More: [docs/bitchx-frontend.md](docs/bitchx-frontend.md),
[docs/bitchx-formats.md](docs/bitchx-formats.md).

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
