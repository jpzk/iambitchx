# BitchX 1.x default formats (reference for frontend = "bitchx")

Read-only source: github.com/BitchX/BitchX1.3 @ master, fetched 2026-11-25
into /workspace/ref/bitchx (not vendored). Files consulted:

- include/color.h  — the "default bx" section (after `#else /* default bx */`,
  non-HUMBLE) holds DEFAULT_STATUS_* and DEFAULT_FORMAT_*_FSET.
- source/status.c  — status expando table (`%T %N %C %+ %F %L %! %W ...`),
  status_notify_windows() (Act: list = refnums of WINDOW_NOTIFIED windows,
  comma-separated, current window only).
- source/vars.c    — STATUS_FORMAT/1/2/3 wiring, DOUBLE_STATUS_LINE.
- include/config.h — DEFAULT_DOUBLE_STATUS_LINE ON, SHOW_STATUS_ALL OFF,
  includes color.h for the defaults.
- source/fset.c    — list of FSET names (PUBLIC, SEND_PUBLIC, JOIN, ...).

## Color codes in FSETs
%n reset, %K dark gray (bold black), %W bright white, %w white, %c cyan,
%C bright cyan, %b blue, %B bright blue, %p magenta, %P bright magenta,
%g green, %G bright green, %r red, %R bright red, %Y bright yellow.
`$G` = SHOW_NUMERICS_STR = bold-red `-`, bold-white `:`, bold-red `-`
(ASCII/LATIN1 build; CP437 build uses box glyphs, we use the ASCII one).

## Status lines (default bx, blue bg = ESC[44m)
STATUS_FORMAT1 (line 1, double status):
  ` [%T][%*%@%G%=%N%#%A]%M [%C%+%W] %Q %H%B%F `
  brackets cyan on blue, time/nick bright white, channel white.
  %+ (STATUS_MODE)   = `(+modes)`   %# (STATUS_UMODE) = `(+umodes)`
  %F (STATUS_NOTIFY) = ` [Act: 2,5]` %A away = ` (zZzZ msg)`
  %W (STATUS_WINDOW) = yellow `^^^^^^^^` only on split screens.
STATUS_FORMAT2 (line 2): ` %L %! %K %>%D ...`
  %L (STATUS_LAG)   = `[Lag N]`
  %! (STATUS_USERS) = `[O/n N/n I/n V/n F/n]` (ops, non-ops, ircops, voice, friends)
  %> right-justifies the rest.
INPUT_PROMPT = `[$C] ` (current channel).
STATUS_USER (status window) = ` * type /help for help `.

## Message FSETs (default bx)
PUBLIC        `%B<%n$nick%B>%n text`      (bright-blue brackets)
SEND_PUBLIC   `%P<%n$mynick%P>%n text`    (own nick: bright-magenta brackets)
ACTION        `%K* %W$nick %ntext`
SEND_ACTION   `%K* %W$mynick %ntext`
JOIN          `$G %C$nick %K[%c$userhost%K]%n has joined $chan`
LEAVE         `$G $nick %K[%w$userhost%K]%n has left $chan %K[%W$reason%K]`
KICK          `$G %n$victim was kicked off $chan by %c$kicker %K(%n$reason%K)`
NICKNAME      `$G %W$old %nis now known as %c$new`
TOPIC         `$G Topic %K(%c$chan%K):%n $topic`
TOPIC_CHANGE  `$G Topic %K(%c$chan%K):%n changed by %c$nick%K:%n $topic`
TOPIC_UNSET   `$G Topic %K(%c$chan%K):%n unset by %c$nick%n`
MODE          `$G %nmode%K/%c$chan %K[%W$modes%K]%n by %W$nick`
NAMES         `$G %K[%GUsers%K(%g$chan%K:%g$n%K/%g$total%K)]%c ...`
NAMES_NICK    `%B$[10]nick`, NAMES_NICK_ME `%W$[10]nick`
WHO           `%Y$[10]chan %W$[10]nick%w %c$[3]flags %w$user%R@%w$host ($real)`

## Matrix mapping (iambitchx)
- nick = own localpart; userhost = full MXID (`@u:server`).
- channel modes: `+i` join_rule invite/knock, `+e`... see screen code;
  ops (`@`, O/) = power level >= 50, voice (`+`, V/) = 0 < PL < 50.
- I/ (ircops) and F/ (friends) have no Matrix analogue: shown as 0.
- Act: = window numbers of rooms with unread messages.
