<div align="center">
    <h1><img width="200" height="200" src="docs/iambitchx.svg"></h1>

[![Build Status](https://github.com/jpzk/iambitchx/actions/workflows/ci.yml/badge.svg)](https://github.com/jpzk/iambitchx/actions?query=workflow%3ACI+)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

</div>

> iambitchx is a fork of [iamb](https://github.com/ulyssa/iamb) by Ulyssa, used under the Apache License 2.0.

## About

`iambitchx` is a Matrix client for the terminal that uses Vim keybindings, with
an optional IRC/BitchX-style frontend (see [docs/bitchx-frontend.md](docs/bitchx-frontend.md)).
It includes support for:

- Threads, spaces, E2EE, and read receipts
- Image previews in terminals that support it (sixels, Kitty, and iTerm2), or using pixelated blocks for those that don't
- Notifications via terminal bell or desktop environment
- Send Markdown, HTML or plaintext messages
- Creating, joining, and leaving rooms
- Sending and accepting room invitations
- Editing, redacting, and reacting to messages
- Custom keybindings
- Multiple profiles
- A BitchX-style status bar, full-screen chat and slash-command input (`frontend = "bitchx"`)

## Documentation

See the manual pages `docs/iambitchx.1` and `docs/iambitchx.5`. Most of the
upstream documentation at [iamb.chat] also applies.

## Configuration

You can create a basic configuration in `$CONFIG_DIR/iambitchx/config.toml` that looks like:

```toml
[profiles."example.com"]
user_id = "@user:example.com"
```

If `$CONFIG_DIR/iambitchx/` does not exist but a legacy `$CONFIG_DIR/iamb/`
directory does, iambitchx keeps using the legacy directory (the same applies to
the data and cache directories), so an existing setup works unchanged.

If your homeserver is located on a different domain than the server part of the
`user_id` and you don't have a [`/.well-known`][well_known_entry] entry, then
you can explicitly specify the homeserver URL to use:

```toml
[profiles."example.com"]
url = "https://example.com"
user_id = "@user:example.com"
```

## Installation (from source)

Install Rust and Cargo using [rustup], and then run from the directory
containing the sources (ie: from a git clone):

```
cargo install --locked --path .
```

If you're willing to wait longer on the build, you can use the `release-lto`
profile to enable link-time optimization:

```
cargo install --profile release-lto --locked --path .
```

If you would prefer to optimize for a smaller binary, you can use the
`release-light` profile, which will do LTO, optimize code generation
for smaller output, and strip the binary. The `max_level_error` feature
will also make sure that any `tracing` logs below the error level have
their strings removed during compile-time, so that you only get `error`
logs.

```
cargo install --profile release-light --features max_level_error --path .
```

Add `--no-default-features` if you want to skip desktop integration (e.g.
clipboard and notifications support) and you would like to link against your
system's SQLite instead of using a bundled SQLite.

### Crate features

- `bundled`: use bundled SQLite instead of system library (default)
- `desktop`: enable desktop notifications and clipboard support (default)
- `max_level_warn`: disable support for more verbose logging than the default `warn` level
- `max_level_error`: disable support for more verbose logging than the `error` level
- `max_level_off`: disable all logs at compile time
- `chafa-dyn`: use [chafa](https://hpjansson.org/chafa/) as a dynamic library for halfblock rendering
- `chafa-static`: use [chafa](https://hpjansson.org/chafa/) as a static library for halfblock rendering

## Installation (via Nix flake)

```
nix profile install "github:jpzk/iambitchx"
```

Distribution packages (Arch, Homebrew, Snap, …) ship upstream iamb, not this
fork; see the upstream README for those.

## License

iambitchx is released under the [Apache License, Version 2.0](LICENSE), like
the upstream project it is forked from.

[iamb.chat]: https://iamb.chat
[well_known_entry]: https://spec.matrix.org/latest/client-server-api/#getwell-knownmatrixclient
