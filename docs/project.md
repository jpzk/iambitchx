# iamBitchX — project notes

Working notes for the project: what it is, how it got here, and the
technical findings worth remembering. Nothing here is marketing copy; it
is the part that would be annoying to rediscover.

## What it is

A BitchX-style Matrix client for the terminal. `/join #channel`, `/msg`,
`/whois`, `/away`, `/set`, `/names`, `/mode` — IRC vocabulary over Matrix,
with end-to-end encryption underneath.

- **base code**: [iamb](https://github.com/ulyssa/iamb) by Ulyssa
  ([iamb.chat](https://iamb.chat)), Apache-2.0. The Matrix backend — sync,
  E2EE, store — is iamb's. The Vim frontend is gone; login and token
  handling were reworked (OAuth device link, proactive refresh).
- **frontend**: new, BitchX-style. See `bitchx-frontend.md`,
  `bitchx-formats.md`, `v2-design.md`.
- **heritage**: the BitchX look and attitude, and the IRC vocabulary. The
  "Why BitchX?" / "History" texts in the README are quoted from
  [prime001/BitchX](https://github.com/prime001/BitchX).

## The splash logo: a 30-year-old rendering bug

The original BitchX 1.3 draws its startup logo (`art.c`,
`do_ansi_logo()`) with **raw CP437 bytes** — `▄ █ ▀ ░ ▓ ▌ ▐` and the
box-drawing set, as octal escapes in the C source. A CP437 terminal
renders it correctly; **every UTF-8 terminal since 1990 shows mojibake**.
Our own recon capture caught the wreckage (the glyphs read as `߲`, `۲`,
`޲` — that is CP437 `▒ ▓ █` mis-decoded as UTF-8).

Two things had to be understood to fix it:

1. **the octal escapes**: `art.c` stores the art as `\334\334\334` etc.
   Decode the C string escapes first, then map CP437 → Unicode.
2. **the ANSI cursor moves**: the 3D letters are *not* in the characters.
   The art is a set of overlapping segments positioned with `ESC[nC`
   jumps. Strip the escapes (what every art-to-text converter does) and
   the logo collapses into noise; replay them onto a canvas and the
   wordmark lands. This is why `src/bitchx/app.rs` carries the art as
   pre-rendered lines: the layout was computed once, offline.

We ship it in two flavours, picked at runtime from the terminal's
capability and size (`Logo::pick`):

| terminal | logo |
|---|---|
| UTF-8, >= 78 cols, >= 36 rows | the original art, decoded to Unicode |
| no UTF-8, roomy | the original "acidjazz" ASCII art |
| either, < 78 cols or < 36 rows | no logo at all (the `[ iamBitchX-ver]` credit stays) |

The size is only known once the terminal exists, so `App::fit_logo()`
re-picks the logo at startup (and on resize) and splices the lines in
place. UTF-8 detection is `TERM` first, locale second — a terminal that
cannot draw is worse than a locale that lies — and it is a **pure
function** of `(term, locale)` so it can be tested. A test that depends
on the ambient `TERM` breaks on CI runners, where `TERM=dumb` is common.

## The history rewrite

The repository history was rewritten to remove personal names from the
codebase *and* from every commit (content, messages, identities). What
that taught us:

- **the rewrite scope is a choice**: our own commits only
  (`fbc4700..`), leaving the upstream iamb base intact so the fork keeps
  merging cleanly. Rewriting upstream would change 488 foreign commits
  and every hash with them.
- **the fixtures constrain the replacement names**: same length *and*
  same sort slot, or `cargo fmt --check` and a dozen tests go red. A
  rename is a semantic change in a codebase with string fixtures.
- **the CI gate belongs before the push**: force-push only after the
  rewritten tree is green, with `--force-with-lease` against the tips
  captured before the rewrite, and a bundle as the rollback.
- **local clones leak too**: rewriting the remote is half the job. The
  local repo, its worktrees, its reflogs and its object database all keep
  the old commits until they are repointed, expired and gc'd.

## Lessons worth keeping

- **"the tests pass" is not "it works"**. The layout test passed while
  the header still said the wrong logo and the art was never rendered.
  Render it, look at it, then assert on the buffer.
- **converting ASCII art to block characters does not work**: one-cell
  strokes either shatter into dashes or turn to mush. Drawing the logo
  in blocks does.
- **replaying the source's own escape sequences does work** — the art was
  never missing, only mis-encoded and mis-positioned.
- **read the licence, don't assume**: BitchX is BSD-3-Clause, not GPL.
  BSD-3 is Apache-2.0 compatible, so the art could be used with
  attribution — `COPYRIGHT-BITCHX` ships with the binaries and the
  artists' credits stay in the artwork (attribution is what the licence
  requires; endorsement is what it forbids).
- **the original's neon is still missing**: the art paints each stroke
  with per-character ANSI colour and background shading. Our renderer
  styles whole lines, so what ships is monochrome. Faithful colour needs
  per-cell styled spans (`model.rs` → `format.rs` → `ui.rs`).

## Tooling

- `bin/bx-ci.sh` — the gate: fmt, import order, clippy, tests.
- `bin/rel-verify.sh` — release artifacts: checksums, SBOM, attestations.
- `docs/ci-supply-chain.md` — what the workflows pin and why.
- `recon/` — the BitchX 1.3 reference material, the recon captures and
  the decoded splash art.
