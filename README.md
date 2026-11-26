<div align="center">
    <h1><img width="200" height="200" src="docs/iambitchx.svg"></h1>

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

- The "Why BitchX?" and "History" texts are taken from
  [prime001/BitchX](https://github.com/prime001/BitchX).
- iamBitchX is a fork of [iamb](https://github.com/ulyssa/iamb) by Ulyssa,
  used under the Apache License 2.0. See [NOTICE](NOTICE) and [LICENSE](LICENSE).
- BitchX is by panasync (Colten Edwards) and contributors. iamBitchX shares no
  code with it, only the look and the attitude.
