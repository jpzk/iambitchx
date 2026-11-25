# Notes For Package Maintainers

## Linking Against System Packages

The default Cargo features for __iamb__ will bundle SQLite. Package maintainers
may want to link against the system's native SQLite instead. To do so, you'll
want to build without the default features, and explicitly add back in `desktop`
(assuming you want notifications and clipboard support):

```
% cargo build --release --no-default-features --features=desktop
```

## Enabling LTO

Enabling LTO can result in smaller binaries. There is a separate profile to
enable it when building:

```
% cargo build --profile release-lto
```

Note that this [can fail][ring-lto] in some build environments if both Clang
and GCC are present.

## Documentation

In addition to the compiled binary, there are other files in the repo that
you'll want to install as part of a package:

<!-- Please keep in sync w/ the `deb`/`generate-rpm` sections of `Cargo.toml` -->
| Repository Path         | Installed Path (may vary per OS)                |
| ----------------------- | ----------------------------------------------- |
| /iambitchx.desktop           | /usr/share/applications/iambitchx.desktop            |
| /config.example.toml    | /usr/share/iambitchx/config.example.toml             |
| /config.full.toml       | /usr/share/iambitchx/config.full.toml                |
| /themes/                | /usr/share/iambitchx/themes/                         |
| /docs/iambitchx-256x256.png  | /usr/share/icons/hicolor/256x256/apps/iambitchx.png  |
| /docs/iambitchx-512x512.png  | /usr/share/icons/hicolor/512x512/apps/iambitchx.png  |
| /docs/iambitchx.svg          | /usr/share/icons/hicolor/scalable/apps/iambitchx.svg |
| /docs/iambitchx.1            | /usr/share/man/man1/iambitchx.1                      |
| /docs/iambitchx.5            | /usr/share/man/man5/iambitchx.5                      |
| /docs/iambitchx.metainfo.xml | /usr/share/metainfo/iambitchx.metainfo.xml           |

[ring-lto]: https://github.com/briansmith/ring/issues/1444
[rustls]: https://crates.io/crates/rustls
