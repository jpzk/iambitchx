//! Splash art and startup banner: the BitchX logos, terminal UTF-8
//! detection, and the startup lines of the status window.

use crate::bitchx::model::{Ink, Line, Run};

/// The original BitchX 1.3 splash logo, from `art.c`
/// (`do_ansi_logo`, case 0), decoded from CP437 to Unicode.
///
/// The original client ships raw CP437 bytes, so a UTF-8 terminal shows
/// mojibake; this is the art as it was meant to render. `{ver}` is the
/// version slot the original filled with its own version string.
///
/// BSD-3-Clause, (c) 1990-2012 the BitchX authors -- see NOTICE.
pub(crate) const BANNER_ORIGINAL: [&str; 23] = [
    "               ",
    "         ▄▄▄                                                                  ",
    "      ▄█▀▀▄▄▄▄▄          ░▓                             ▐▄   ▄▄ ",
    "        ▄▄█▀▀▀   ▄▄▄▄▄█▀▀▀                             ▀       ▀▀▀▀█▄▄▄▄ ▄▄▄",
    "       ▀▀ ▄▄▄▄▀▀▓█▀ ▀                                               ▀▀▓▓▄██▀▀",
    " █░░██ ▄▄██▓▌▀ ▐█▌ ▌        ▄▄▄▄      ▐  ██              ▐████   ▐▄▄▄▐▐██▀▀  ",
    " █▓▓▄▄▄▄▄▄▄▄▄▄▄ ▀▀▀█▄    ▄▄██▓▀██▄▄   █░░░█       ▄▄▄     ██░░░  ▐▐▀████▄▄▄",
    " ███▀▌▌     ▀▀▀██▄▄▄▄ ▀ ▐▐█▓██▄▄▐█▌▌ ▐▐▓▓▌▌▄▄▄▄█▓█▓░      ▐░▓▓▓▌▌ █▌▀▓▓███▀▀▀",
    " ▐▐▀▐▌▌▀▌       ▀▀████▄▄ ▀▀▀▀▐███▀▀  ████▌ ▀   ███▌▌ ▄▄▄▄██ ██▓▌▌ █▌ ▄▀▀▀ ▄▄█",
    "▄▐▐▄░██ ▌▄░▓     ▐▐▓▓█▓░▌   ▀▀▀▀ ▄▄ ▐▐███ ▌   ▐▐██▄▓▓█▀▀▀  ▐▐████ ▐▀▀ ▄▄▄██▄▄▄",
    "▌▐▐█▓█  ▀▀      ▄█▀▀▀▀   ▄▄▄▄▄███▌▌  █████   ▄▄██▄█▀▀       ████░ ▄▄▐▀▀▀▀███▓░",
    "▌ █████      ▀▀          ▀▀▐▐▌ ▄██ ▐▐█▓█▌▌  ▐▐█▌▀█▌▌   Pe^  ▐▐▓▓██▀▀    ██▓█▌▌",
    "░ ███▀▀▀▀▀▀██▄▄▄▄▄▄        ▐▐▌░█▌▌ ██▌░█▌▌   ▀▀█▄▄█▄▄        ████      ▐▐▀░█ ▄",
    "░ ████▌▌  ▄▄▄ ▀▀▀▀██▀█▄▄▄  ██▄▓█▌▌▐▐█ ▐██ ▄▐    ▀▀▀███▄▄▄    ▐▐██▌▌ ░▓ ▐▐ ▐▌▌▌",
    "  ▐▐▀▓▌▌    ▀░▄▄▄▄ ▀▀█▄ ▀▀███████▄▄▓▀████ █▄   ▐▐████▀▀███▄▄▄  ██▌▌    █████▄▌",
    "  ▐▐▓██▄▄    ▐▐▓▀▌▌ ▐▐█▌ ░▓▓██████████▀▀▀     ▄▄███▀▀ ▄ ▀▀▀███▄▐███    █▀▀ ▌░▓",
    "  ░▓░▌▀▀▀    ▀▀▀   ▄▄█▀█▓▓█▀▀▀▀    ▀▀▀▀█▄▄▄▄▄█▀▀▀▀ ▄▄▄▀█▄▄  ▀▀██▀██    █▄    ",
    "  ░▓         ▄▄▄▄▄███▌▌▐▀▀              ▀▀██▄▄  ▀█▀█▄ ░▓▓░▌  ▐▐▓▓▓░▌    ",
    "                       ▐▌    PhonyEye    ▐▐█▓░▌  ▐ ▐▀█▓█▀▀             ",
    "    [%15s]  ▐▀    ^          ▄▄█▀▀▀  ▄█ ▀▌▀▀░▓        Awe/Cia",
    "                       ▀             ██ ▀           ▌",
    "                                                    ▀                 ",
    "                       ",
];

pub(crate) const BANNER_ORIGINAL_INK: [&[(u8, Ink, Ink, bool)]; 23] = [
    &[(12, Ink::Magenta, Ink::Default, false)],
    &[
        (4, Ink::Magenta, Ink::Default, false),
        (12, Ink::Default, Ink::Default, false),
        (45, Ink::Magenta, Ink::Default, false),
        (48, Ink::Default, Ink::Default, false),
        (76, Ink::Blue, Ink::Default, false),
    ],
    &[
        (0, Ink::Magenta, Ink::Default, false),
        (8, Ink::Magenta, Ink::Default, true),
        (9, Ink::Magenta, Ink::Default, false),
        (15, Ink::Default, Ink::Default, false),
        (25, Ink::Magenta, Ink::Default, false),
        (26, Ink::Black, Ink::Magenta, false),
        (27, Ink::Default, Ink::Default, false),
        (32, Ink::Cyan, Ink::Black, false),
        (48, Ink::Default, Ink::Default, false),
        (52, Ink::Magenta, Ink::Black, false),
        (54, Ink::White, Ink::Black, false),
        (56, Ink::Magenta, Ink::Magenta, true),
        (57, Ink::Magenta, Ink::Black, true),
        (58, Ink::Default, Ink::Default, false),
        (61, Ink::Magenta, Ink::Default, false),
        (62, Ink::Magenta, Ink::Default, true),
        (63, Ink::Magenta, Ink::Default, false),
    ],
    &[
        (4, Ink::Magenta, Ink::Default, false),
        (9, Ink::Magenta, Ink::Default, true),
        (10, Ink::Magenta, Ink::Magenta, true),
        (11, Ink::Magenta, Ink::Black, true),
        (13, Ink::Magenta, Ink::Default, false),
        (20, Ink::Magenta, Ink::Default, true),
        (22, Ink::Magenta, Ink::Default, false),
        (27, Ink::White, Ink::Default, false),
        (28, Ink::Magenta, Ink::Default, false),
        (30, Ink::Default, Ink::Default, false),
        (34, Ink::Cyan, Ink::Default, false),
        (48, Ink::Default, Ink::Default, false),
        (55, Ink::Magenta, Ink::Default, false),
        (56, Ink::Default, Ink::Default, false),
        (62, Ink::Magenta, Ink::Default, false),
        (64, Ink::Magenta, Ink::Default, true),
        (66, Ink::Magenta, Ink::Magenta, true),
        (69, Ink::Magenta, Ink::Black, true),
        (71, Ink::Magenta, Ink::Default, false),
        (72, Ink::White, Ink::Default, false),
        (73, Ink::Magenta, Ink::Default, false),
        (74, Ink::Magenta, Ink::Magenta, true),
        (75, Ink::Magenta, Ink::Default, false),
    ],
    &[
        (6, Ink::Magenta, Ink::Default, false),
        (11, Ink::Magenta, Ink::Default, true),
        (13, Ink::Magenta, Ink::Magenta, true),
        (14, Ink::Magenta, Ink::White, true),
        (17, Ink::Magenta, Ink::Magenta, true),
        (19, Ink::Red, Ink::Magenta, true),
        (20, Ink::Magenta, Ink::Default, false),
        (21, Ink::Default, Ink::Default, false),
        (28, Ink::Magenta, Ink::Default, false),
        (29, Ink::Default, Ink::Default, false),
        (33, Ink::Magenta, Ink::Default, false),
        (37, Ink::Default, Ink::Default, false),
        (42, Ink::Magenta, Ink::Default, false),
        (47, Ink::Default, Ink::Default, false),
        (68, Ink::Magenta, Ink::Default, false),
        (69, Ink::Magenta, Ink::Magenta, true),
        (70, Ink::Magenta, Ink::White, true),
        (72, Ink::Magenta, Ink::Magenta, true),
        (76, Ink::Magenta, Ink::Default, false),
    ],
    &[
        (1, Ink::Blue, Ink::Default, false),
        (2, Ink::Blue, Ink::Blue, true),
        (4, Ink::Blue, Ink::Default, false),
        (6, Ink::White, Ink::Default, false),
        (7, Ink::Magenta, Ink::Default, false),
        (8, Ink::Magenta, Ink::Magenta, true),
        (10, Ink::Magenta, Ink::White, true),
        (16, Ink::Magenta, Ink::Magenta, true),
        (18, Ink::Red, Ink::Magenta, true),
        (19, Ink::Magenta, Ink::Default, false),
        (20, Ink::Blue, Ink::Default, false),
        (23, Ink::Default, Ink::Default, false),
        (28, Ink::Blue, Ink::Default, false),
        (29, Ink::Blue, Ink::Default, true),
        (31, Ink::Blue, Ink::Default, false),
        (32, Ink::White, Ink::Default, false),
        (33, Ink::Magenta, Ink::Default, false),
        (37, Ink::White, Ink::Default, false),
        (38, Ink::Blue, Ink::Default, false),
        (39, Ink::Blue, Ink::Blue, true),
        (41, Ink::Blue, Ink::Default, false),
        (43, Ink::White, Ink::Default, false),
        (44, Ink::Magenta, Ink::Default, false),
        (45, Ink::Default, Ink::Default, false),
        (57, Ink::Blue, Ink::Default, false),
        (62, Ink::Blue, Ink::Blue, true),
        (64, Ink::Blue, Ink::Default, false),
        (65, Ink::Magenta, Ink::Default, false),
        (66, Ink::Magenta, Ink::Default, true),
        (68, Ink::Magenta, Ink::Default, false),
        (70, Ink::Magenta, Ink::Magenta, true),
        (74, Ink::Magenta, Ink::Default, false),
    ],
    &[
        (1, Ink::Blue, Ink::Default, false),
        (2, Ink::Blue, Ink::Blue, true),
        (4, Ink::Blue, Ink::Black, true),
        (14, Ink::Blue, Ink::Default, false),
        (15, Ink::White, Ink::Default, false),
        (16, Ink::Magenta, Ink::Default, true),
        (18, Ink::Magenta, Ink::Default, false),
        (21, Ink::White, Ink::Default, false),
        (22, Ink::Blue, Ink::Default, false),
        (24, Ink::White, Ink::Default, false),
        (25, Ink::Blue, Ink::Default, false),
        (26, Ink::Blue, Ink::Blue, true),
        (29, Ink::Blue, Ink::White, true),
        (31, Ink::Blue, Ink::Blue, true),
        (34, Ink::Blue, Ink::Default, false),
        (35, Ink::White, Ink::Default, false),
        (36, Ink::Magenta, Ink::Default, false),
        (37, Ink::White, Ink::Default, false),
        (38, Ink::Blue, Ink::Default, false),
        (39, Ink::Blue, Ink::Blue, true),
        (42, Ink::Blue, Ink::Default, false),
        (43, Ink::White, Ink::Default, false),
        (44, Ink::Magenta, Ink::Default, false),
        (47, Ink::Default, Ink::Default, false),
        (50, Ink::Blue, Ink::Default, false),
        (53, Ink::Default, Ink::Default, false),
        (58, Ink::Blue, Ink::Default, false),
        (60, Ink::Blue, Ink::Blue, true),
        (64, Ink::Blue, Ink::Default, false),
        (65, Ink::Magenta, Ink::Default, false),
        (66, Ink::Magenta, Ink::Magenta, true),
        (67, Ink::Magenta, Ink::White, true),
        (68, Ink::Magenta, Ink::Magenta, true),
        (69, Ink::Magenta, Ink::White, true),
        (72, Ink::Magenta, Ink::Magenta, true),
        (73, Ink::Magenta, Ink::Black, true),
        (74, Ink::Magenta, Ink::Default, false),
    ],
    &[
        (1, Ink::Blue, Ink::Default, false),
        (2, Ink::Blue, Ink::Blue, true),
        (4, Ink::Blue, Ink::White, true),
        (5, Ink::Blue, Ink::Blue, true),
        (6, Ink::Blue, Ink::Default, false),
        (7, Ink::Default, Ink::Default, false),
        (12, Ink::Blue, Ink::Default, false),
        (13, Ink::Blue, Ink::Default, true),
        (14, Ink::Blue, Ink::Blue, true),
        (18, Ink::Blue, Ink::Black, true),
        (20, Ink::Blue, Ink::Default, false),
        (21, Ink::White, Ink::Default, false),
        (22, Ink::Magenta, Ink::Default, false),
        (23, Ink::White, Ink::Default, false),
        (24, Ink::Blue, Ink::Default, false),
        (25, Ink::Blue, Ink::Blue, true),
        (29, Ink::Blue, Ink::White, true),
        (33, Ink::Blue, Ink::Blue, true),
        (35, Ink::Blue, Ink::Default, false),
        (36, Ink::White, Ink::Default, false),
        (37, Ink::Blue, Ink::Default, false),
        (38, Ink::Blue, Ink::Blue, true),
        (42, Ink::Blue, Ink::Default, false),
        (43, Ink::Blue, Ink::Default, true),
        (46, Ink::Blue, Ink::Blue, true),
        (48, Ink::Blue, Ink::Cyan, true),
        (49, Ink::Blue, Ink::Blue, true),
        (52, Ink::Default, Ink::Default, false),
        (57, Ink::Blue, Ink::Default, true),
        (58, Ink::Blue, Ink::Default, false),
        (59, Ink::Blue, Ink::Blue, true),
        (62, Ink::Blue, Ink::Black, true),
        (63, Ink::Blue, Ink::Blue, true),
        (64, Ink::Blue, Ink::Default, false),
        (65, Ink::White, Ink::Default, false),
        (66, Ink::Magenta, Ink::Default, false),
        (67, Ink::Magenta, Ink::White, true),
        (71, Ink::Magenta, Ink::Black, true),
        (74, Ink::Magenta, Ink::Magenta, true),
        (75, Ink::Magenta, Ink::Black, true),
        (76, Ink::Magenta, Ink::Default, false),
    ],
    &[
        (1, Ink::Blue, Ink::Default, false),
        (2, Ink::Blue, Ink::Blue, true),
        (3, Ink::Blue, Ink::White, true),
        (5, Ink::Blue, Ink::Blue, true),
        (6, Ink::Blue, Ink::Default, false),
        (7, Ink::Magenta, Ink::Default, false),
        (9, Ink::Default, Ink::Default, false),
        (16, Ink::Blue, Ink::Default, false),
        (17, Ink::Blue, Ink::Blue, true),
        (23, Ink::Blue, Ink::Default, false),
        (26, Ink::Blue, Ink::Blue, true),
        (31, Ink::Blue, Ink::White, true),
        (33, Ink::Blue, Ink::Blue, true),
        (34, Ink::Blue, Ink::Default, false),
        (38, Ink::Blue, Ink::Blue, true),
        (43, Ink::Blue, Ink::Black, true),
        (44, Ink::Blue, Ink::Default, false),
        (46, Ink::White, Ink::Default, false),
        (47, Ink::Blue, Ink::Default, false),
        (48, Ink::Blue, Ink::Blue, true),
        (51, Ink::Blue, Ink::Default, false),
        (52, Ink::White, Ink::Default, false),
        (53, Ink::Blue, Ink::Default, false),
        (54, Ink::Blue, Ink::Default, true),
        (57, Ink::Black, Ink::Blue, false),
        (59, Ink::Blue, Ink::Blue, true),
        (60, Ink::Blue, Ink::Black, true),
        (61, Ink::Blue, Ink::Blue, true),
        (64, Ink::Blue, Ink::Default, false),
        (66, Ink::Magenta, Ink::Default, false),
        (67, Ink::Magenta, Ink::White, true),
        (70, Ink::Magenta, Ink::Magenta, true),
        (71, Ink::Magenta, Ink::Black, true),
        (72, Ink::Magenta, Ink::Default, false),
        (73, Ink::Blue, Ink::Default, false),
    ],
    &[
        (0, Ink::Magenta, Ink::Default, false),
        (1, Ink::Blue, Ink::Default, false),
        (2, Ink::Blue, Ink::Blue, true),
        (3, Ink::Blue, Ink::White, true),
        (5, Ink::Blue, Ink::Black, true),
        (6, Ink::Blue, Ink::Default, false),
        (7, Ink::Magenta, Ink::Default, false),
        (8, Ink::Magenta, Ink::Default, true),
        (9, Ink::Magenta, Ink::Default, false),
        (11, Ink::Black, Ink::Magenta, false),
        (12, Ink::Default, Ink::Default, false),
        (17, Ink::Blue, Ink::Black, false),
        (18, Ink::Blue, Ink::Blue, true),
        (19, Ink::Blue, Ink::White, true),
        (21, Ink::Blue, Ink::Blue, true),
        (24, Ink::Blue, Ink::Default, false),
        (37, Ink::Blue, Ink::Blue, true),
        (39, Ink::Blue, Ink::White, true),
        (40, Ink::Blue, Ink::Black, true),
        (41, Ink::Blue, Ink::Blue, true),
        (42, Ink::Blue, Ink::Default, false),
        (47, Ink::Blue, Ink::Blue, true),
        (51, Ink::Blue, Ink::White, true),
        (53, Ink::Blue, Ink::Blue, true),
        (55, Ink::Blue, Ink::Black, true),
        (57, Ink::Default, Ink::Default, false),
        (59, Ink::Blue, Ink::Default, false),
        (60, Ink::Blue, Ink::Blue, true),
        (64, Ink::Blue, Ink::Default, false),
        (65, Ink::White, Ink::Default, false),
        (66, Ink::Magenta, Ink::Default, false),
        (67, Ink::Magenta, Ink::Default, true),
        (68, Ink::Magenta, Ink::Default, false),
        (69, Ink::White, Ink::Default, false),
        (70, Ink::Blue, Ink::Default, false),
        (71, Ink::Blue, Ink::Default, true),
        (72, Ink::Blue, Ink::Blue, true),
        (76, Ink::Blue, Ink::Black, true),
        (77, Ink::Blue, Ink::Default, false),
    ],
    &[
        (0, Ink::Magenta, Ink::Default, false),
        (1, Ink::Blue, Ink::Default, false),
        (2, Ink::Blue, Ink::Blue, true),
        (3, Ink::Blue, Ink::White, true),
        (6, Ink::Blue, Ink::Blue, true),
        (7, Ink::Default, Ink::Default, false),
        (8, Ink::Magenta, Ink::Default, true),
        (9, Ink::Magenta, Ink::Default, false),
        (10, Ink::Default, Ink::Default, false),
        (13, Ink::Blue, Ink::Default, false),
        (18, Ink::Blue, Ink::Blue, true),
        (19, Ink::Blue, Ink::Black, true),
        (20, Ink::Blue, Ink::Default, false),
        (24, Ink::White, Ink::Default, false),
        (25, Ink::Blue, Ink::Default, false),
        (26, Ink::Blue, Ink::Default, true),
        (29, Ink::Blue, Ink::Blue, true),
        (34, Ink::Blue, Ink::Default, false),
        (35, Ink::White, Ink::Default, false),
        (36, Ink::Blue, Ink::Blue, true),
        (41, Ink::Blue, Ink::Default, false),
        (42, Ink::White, Ink::Default, false),
        (44, Ink::Blue, Ink::Default, false),
        (46, Ink::Blue, Ink::Blue, true),
        (48, Ink::Blue, Ink::White, true),
        (50, Ink::Blue, Ink::Blue, true),
        (52, Ink::Blue, Ink::Default, false),
        (54, Ink::White, Ink::Default, false),
        (56, Ink::Magenta, Ink::Default, false),
        (59, Ink::Blue, Ink::Default, false),
        (61, Ink::Blue, Ink::Blue, true),
        (65, Ink::Default, Ink::Default, false),
        (66, Ink::Blue, Ink::Default, false),
        (68, Ink::Blue, Ink::Blue, true),
        (70, Ink::Blue, Ink::Black, true),
        (72, Ink::Blue, Ink::Blue, true),
    ],
    &[
        (0, Ink::Magenta, Ink::Default, false),
        (2, Ink::Blue, Ink::Default, false),
        (3, Ink::Blue, Ink::White, true),
        (5, Ink::Blue, Ink::Blue, true),
        (6, Ink::Blue, Ink::Default, false),
        (7, Ink::Default, Ink::Default, false),
        (13, Ink::Blue, Ink::Default, false),
        (15, Ink::Default, Ink::Default, false),
        (25, Ink::Blue, Ink::Default, false),
        (26, Ink::Blue, Ink::Default, true),
        (27, Ink::Blue, Ink::Default, false),
        (28, Ink::Blue, Ink::Blue, true),
        (29, Ink::Blue, Ink::White, true),
        (33, Ink::Blue, Ink::Default, false),
        (34, Ink::White, Ink::Default, false),
        (35, Ink::Blue, Ink::Default, false),
        (36, Ink::Blue, Ink::Blue, true),
        (38, Ink::Blue, Ink::White, true),
        (40, Ink::Blue, Ink::Blue, true),
        (41, Ink::Blue, Ink::Default, false),
        (42, Ink::White, Ink::Default, false),
        (43, Ink::Blue, Ink::Default, false),
        (45, Ink::Blue, Ink::Blue, true),
        (46, Ink::Blue, Ink::White, true),
        (49, Ink::Blue, Ink::Blue, true),
        (51, Ink::Blue, Ink::Default, false),
        (52, Ink::White, Ink::Default, false),
        (53, Ink::Magenta, Ink::Default, false),
        (54, Ink::Black, Ink::Default, true),
        (58, Ink::Magenta, Ink::Default, false),
        (60, Ink::Blue, Ink::Default, false),
        (61, Ink::Blue, Ink::Blue, true),
        (62, Ink::Blue, Ink::White, true),
        (64, Ink::Blue, Ink::Blue, true),
        (67, Ink::Blue, Ink::Default, false),
        (69, Ink::White, Ink::Default, false),
        (71, Ink::Blue, Ink::Default, false),
        (73, Ink::Blue, Ink::Blue, true),
        (74, Ink::Blue, Ink::White, true),
        (75, Ink::Blue, Ink::Blue, true),
        (77, Ink::Blue, Ink::Default, false),
    ],
    &[
        (0, Ink::Black, Ink::Magenta, false),
        (1, Ink::White, Ink::Black, false),
        (2, Ink::Blue, Ink::Black, false),
        (3, Ink::Blue, Ink::Blue, true),
        (7, Ink::Blue, Ink::Black, true),
        (10, Ink::Blue, Ink::Blue, true),
        (14, Ink::Blue, Ink::Black, true),
        (18, Ink::Blue, Ink::Default, false),
        (19, Ink::Default, Ink::Default, false),
        (27, Ink::Blue, Ink::Default, false),
        (28, Ink::Blue, Ink::Blue, true),
        (29, Ink::Blue, Ink::White, true),
        (31, Ink::Blue, Ink::Blue, true),
        (33, Ink::Blue, Ink::Default, false),
        (34, Ink::White, Ink::Default, false),
        (35, Ink::Blue, Ink::Default, false),
        (36, Ink::Blue, Ink::Blue, true),
        (37, Ink::Blue, Ink::White, true),
        (40, Ink::Blue, Ink::Blue, true),
        (41, Ink::Blue, Ink::Default, false),
        (42, Ink::White, Ink::Default, false),
        (43, Ink::Magenta, Ink::Default, false),
        (44, Ink::Blue, Ink::Default, false),
        (46, Ink::Blue, Ink::Blue, true),
        (48, Ink::Blue, Ink::White, true),
        (51, Ink::Blue, Ink::Blue, true),
        (52, Ink::Blue, Ink::Default, false),
        (53, Ink::White, Ink::Default, false),
        (54, Ink::Magenta, Ink::Default, false),
        (60, Ink::White, Ink::Default, false),
        (61, Ink::Blue, Ink::Default, false),
        (62, Ink::Blue, Ink::Blue, true),
        (66, Ink::Default, Ink::Default, false),
        (67, Ink::Blue, Ink::Default, false),
        (69, Ink::White, Ink::Default, false),
        (71, Ink::Blue, Ink::Default, false),
        (72, Ink::Blue, Ink::Blue, true),
        (73, Ink::Blue, Ink::White, true),
        (75, Ink::Blue, Ink::Blue, true),
        (77, Ink::Magenta, Ink::Default, false),
    ],
    &[
        (0, Ink::Magenta, Ink::Default, false),
        (1, Ink::White, Ink::Default, false),
        (2, Ink::Blue, Ink::Default, false),
        (3, Ink::Blue, Ink::Blue, true),
        (7, Ink::Blue, Ink::Default, false),
        (8, Ink::White, Ink::Default, false),
        (10, Ink::Magenta, Ink::Default, false),
        (13, Ink::White, Ink::Default, false),
        (14, Ink::Blue, Ink::Default, false),
        (15, Ink::Blue, Ink::Default, true),
        (17, Ink::Blue, Ink::Blue, true),
        (19, Ink::Blue, Ink::White, true),
        (22, Ink::Blue, Ink::Blue, true),
        (23, Ink::Blue, Ink::Black, true),
        (25, Ink::Default, Ink::Default, false),
        (27, Ink::Blue, Ink::Default, false),
        (28, Ink::Blue, Ink::Blue, true),
        (29, Ink::Blue, Ink::White, true),
        (31, Ink::Blue, Ink::Blue, true),
        (33, Ink::Blue, Ink::Default, false),
        (35, Ink::Blue, Ink::Blue, true),
        (37, Ink::Blue, Ink::White, true),
        (39, Ink::Blue, Ink::Blue, true),
        (40, Ink::Blue, Ink::Default, false),
        (41, Ink::White, Ink::Default, false),
        (42, Ink::Magenta, Ink::Default, false),
        (45, Ink::White, Ink::Default, false),
        (46, Ink::Blue, Ink::Default, false),
        (49, Ink::Blue, Ink::Default, true),
        (50, Ink::Blue, Ink::Blue, true),
        (52, Ink::Blue, Ink::Black, true),
        (54, Ink::Blue, Ink::Blue, true),
        (55, Ink::Blue, Ink::Black, true),
        (56, Ink::Blue, Ink::Default, false),
        (57, Ink::White, Ink::Default, false),
        (58, Ink::Magenta, Ink::Default, false),
        (59, Ink::White, Ink::Default, false),
        (61, Ink::Blue, Ink::Default, false),
        (62, Ink::Blue, Ink::Blue, true),
        (66, Ink::Blue, Ink::Default, false),
        (67, Ink::White, Ink::Default, false),
        (68, Ink::Magenta, Ink::Default, false),
        (69, Ink::Black, Ink::Magenta, false),
        (70, Ink::White, Ink::Black, false),
        (71, Ink::Blue, Ink::Black, false),
        (72, Ink::Blue, Ink::Blue, true),
        (73, Ink::Blue, Ink::White, true),
        (75, Ink::Blue, Ink::Blue, true),
        (76, Ink::Blue, Ink::Default, false),
        (77, Ink::Magenta, Ink::Default, false),
    ],
    &[
        (2, Ink::Blue, Ink::Default, false),
        (3, Ink::Blue, Ink::Blue, true),
        (4, Ink::Blue, Ink::Cyan, true),
        (6, Ink::Blue, Ink::Blue, true),
        (7, Ink::Blue, Ink::Default, false),
        (8, Ink::Default, Ink::Default, false),
        (12, Ink::Magenta, Ink::Default, false),
        (13, Ink::Magenta, Ink::Magenta, true),
        (16, Ink::Magenta, Ink::Default, false),
        (18, Ink::White, Ink::Default, false),
        (19, Ink::Blue, Ink::Default, false),
        (20, Ink::Blue, Ink::Blue, true),
        (22, Ink::Blue, Ink::White, true),
        (27, Ink::Blue, Ink::Blue, true),
        (35, Ink::Blue, Ink::White, true),
        (37, Ink::Blue, Ink::Blue, true),
        (40, Ink::Blue, Ink::Default, false),
        (42, Ink::Magenta, Ink::Default, false),
        (44, Ink::Default, Ink::Default, false),
        (47, Ink::Blue, Ink::Default, false),
        (48, Ink::Blue, Ink::Blue, true),
        (59, Ink::Blue, Ink::Black, true),
        (60, Ink::Blue, Ink::Default, false),
        (62, Ink::Blue, Ink::Blue, true),
        (66, Ink::Blue, Ink::Default, false),
        (67, Ink::Default, Ink::Default, false),
        (71, Ink::Blue, Ink::Default, false),
        (72, Ink::Blue, Ink::Blue, true),
        (75, Ink::Blue, Ink::Default, false),
        (76, Ink::Magenta, Ink::Default, false),
    ],
    &[
        (2, Ink::Blue, Ink::Default, false),
        (3, Ink::Blue, Ink::Blue, true),
        (8, Ink::Blue, Ink::Default, false),
        (10, Ink::White, Ink::Default, false),
        (12, Ink::Magenta, Ink::Default, false),
        (14, Ink::Magenta, Ink::Magenta, true),
        (15, Ink::Magenta, Ink::White, true),
        (16, Ink::Magenta, Ink::Magenta, true),
        (18, Ink::Magenta, Ink::Default, false),
        (19, Ink::White, Ink::Default, false),
        (20, Ink::Blue, Ink::Default, false),
        (21, Ink::Blue, Ink::Blue, true),
        (22, Ink::Blue, Ink::White, true),
        (28, Ink::Blue, Ink::Blue, true),
        (30, Ink::Blue, Ink::Black, true),
        (33, Ink::Blue, Ink::Blue, true),
        (38, Ink::Blue, Ink::Default, false),
        (47, Ink::Blue, Ink::Blue, true),
        (52, Ink::Blue, Ink::Default, false),
        (53, Ink::White, Ink::Default, false),
        (54, Ink::Magenta, Ink::Default, false),
        (55, Ink::White, Ink::Default, false),
        (56, Ink::Blue, Ink::Default, false),
        (57, Ink::Blue, Ink::Default, true),
        (58, Ink::Blue, Ink::Blue, true),
        (66, Ink::Blue, Ink::Default, false),
        (67, Ink::White, Ink::Default, false),
        (68, Ink::Blue, Ink::Default, false),
        (69, Ink::White, Ink::Default, false),
        (71, Ink::Blue, Ink::Default, false),
        (74, Ink::Blue, Ink::Blue, true),
        (75, Ink::Blue, Ink::Default, false),
        (76, Ink::Magenta, Ink::Default, false),
        (77, Ink::Black, Ink::Magenta, false),
    ],
    &[
        (2, Ink::Black, Ink::Blue, false),
        (3, Ink::Blue, Ink::Black, false),
        (4, Ink::Blue, Ink::Blue, true),
        (5, Ink::Blue, Ink::Default, false),
        (7, Ink::Blue, Ink::Default, true),
        (8, Ink::Blue, Ink::Default, false),
        (10, Ink::White, Ink::Default, false),
        (11, Ink::Magenta, Ink::Default, false),
        (12, Ink::White, Ink::Default, false),
        (13, Ink::Magenta, Ink::Default, false),
        (14, Ink::Magenta, Ink::Default, true),
        (16, Ink::Default, Ink::Default, false),
        (19, Ink::Blue, Ink::Default, false),
        (20, Ink::Blue, Ink::Blue, true),
        (24, Ink::Blue, Ink::White, true),
        (26, Ink::Blue, Ink::Blue, true),
        (28, Ink::Blue, Ink::Black, true),
        (30, Ink::Blue, Ink::Default, false),
        (34, Ink::White, Ink::Default, false),
        (35, Ink::Blue, Ink::Default, false),
        (37, Ink::Blue, Ink::Default, true),
        (38, Ink::Blue, Ink::Blue, true),
        (41, Ink::Blue, Ink::Black, true),
        (44, Ink::Blue, Ink::Blue, true),
        (47, Ink::Blue, Ink::Black, true),
        (49, Ink::Blue, Ink::Default, false),
        (51, Ink::Magenta, Ink::Default, false),
        (52, Ink::Magenta, Ink::Default, true),
        (53, Ink::Magenta, Ink::Magenta, true),
        (54, Ink::Magenta, Ink::White, true),
        (55, Ink::Magenta, Ink::Magenta, true),
        (57, Ink::Magenta, Ink::Default, false),
        (58, Ink::White, Ink::Default, false),
        (60, Ink::Blue, Ink::Default, false),
        (61, Ink::Blue, Ink::Blue, true),
        (64, Ink::Blue, Ink::Cyan, true),
        (65, Ink::Blue, Ink::Blue, true),
        (66, Ink::Blue, Ink::Default, false),
        (69, Ink::White, Ink::Default, false),
        (70, Ink::Blue, Ink::Default, false),
    ],
    &[
        (2, Ink::Blue, Ink::Default, false),
        (3, Ink::Black, Ink::Blue, false),
        (4, Ink::Blue, Ink::Black, false),
        (10, Ink::White, Ink::Black, false),
        (11, Ink::Blue, Ink::Black, false),
        (12, Ink::White, Ink::Black, false),
        (13, Ink::Blue, Ink::Black, false),
        (14, Ink::Blue, Ink::Black, true),
        (17, Ink::Blue, Ink::Blue, true),
        (18, Ink::Blue, Ink::Black, true),
        (21, Ink::Blue, Ink::Blue, true),
        (22, Ink::Blue, Ink::Default, false),
        (23, Ink::Blue, Ink::Blue, true),
        (25, Ink::Blue, Ink::Default, false),
        (26, Ink::White, Ink::Default, false),
        (27, Ink::Blue, Ink::Default, false),
        (35, Ink::Default, Ink::Default, false),
        (40, Ink::Blue, Ink::Default, false),
        (41, Ink::Blue, Ink::Blue, true),
        (45, Ink::Blue, Ink::Default, false),
        (48, Ink::Magenta, Ink::Default, false),
        (50, Ink::Magenta, Ink::Magenta, true),
        (51, Ink::Magenta, Ink::White, true),
        (56, Ink::Magenta, Ink::Magenta, true),
        (58, Ink::Magenta, Ink::Default, false),
        (60, Ink::White, Ink::Default, false),
        (61, Ink::Blue, Ink::Default, false),
        (62, Ink::Blue, Ink::Blue, true),
        (63, Ink::Blue, Ink::Cyan, true),
        (65, Ink::Blue, Ink::Blue, true),
        (67, Ink::Blue, Ink::Default, false),
        (68, Ink::White, Ink::Default, false),
        (70, Ink::Blue, Ink::Default, false),
    ],
    &[
        (2, Ink::Blue, Ink::Default, false),
        (3, Ink::Default, Ink::Default, false),
        (17, Ink::Magenta, Ink::Default, false),
        (22, Ink::White, Ink::Default, false),
        (23, Ink::Blue, Ink::Blue, true),
        (24, Ink::Blue, Ink::Default, false),
        (25, Ink::Default, Ink::Default, false),
        (29, Ink::Black, Ink::Default, true),
        (30, Ink::Black, Ink::Red, true),
        (31, Ink::Black, Ink::Black, true),
        (37, Ink::Blue, Ink::Default, false),
        (40, Ink::White, Ink::Default, false),
        (41, Ink::Blue, Ink::Default, false),
        (42, Ink::Blue, Ink::Blue, true),
        (46, Ink::Blue, Ink::Default, false),
        (49, Ink::Magenta, Ink::Default, false),
        (50, Ink::Magenta, Ink::Magenta, true),
        (54, Ink::Magenta, Ink::White, true),
        (55, Ink::Magenta, Ink::Magenta, true),
        (57, Ink::Magenta, Ink::Default, false),
        (58, Ink::Blue, Ink::Default, false),
        (61, Ink::Magenta, Ink::Default, false),
        (70, Ink::Blue, Ink::Default, false),
    ],
    &[
        (4, Ink::Blue, Ink::Default, false),
        (5, Ink::White, Ink::Default, false),
        (9, Ink::Blue, Ink::Default, false),
        (10, Ink::Magenta, Ink::Default, false),
        (11, Ink::White, Ink::Default, false),
        (12, Ink::Blue, Ink::Default, true),
        (13, Ink::Blue, Ink::Default, false),
        (14, Ink::Default, Ink::Default, false),
        (18, Ink::Black, Ink::Default, true),
        (19, Ink::Default, Ink::Default, false),
        (29, Ink::Blue, Ink::Default, false),
        (30, Ink::Blue, Ink::Blue, true),
        (33, Ink::Blue, Ink::Black, true),
        (34, Ink::Blue, Ink::Default, false),
        (35, Ink::White, Ink::Default, false),
        (37, Ink::Magenta, Ink::Default, false),
        (39, Ink::Magenta, Ink::Magenta, true),
        (41, Ink::Magenta, Ink::Default, false),
        (45, Ink::Black, Ink::Magenta, false),
        (46, Ink::Default, Ink::Default, false),
        (54, Ink::Black, Ink::Black, true),
    ],
    &[
        (10, Ink::Magenta, Ink::Default, false),
        (20, Ink::Blue, Ink::Default, false),
        (23, Ink::Blue, Ink::Default, true),
        (24, Ink::Blue, Ink::Default, false),
        (28, Ink::Default, Ink::Default, false),
        (37, Ink::Blue, Ink::Default, false),
        (41, Ink::Default, Ink::Default, false),
        (46, Ink::Magenta, Ink::Default, false),
        (49, Ink::Blue, Ink::Default, false),
        (52, Ink::Magenta, Ink::Default, false),
    ],
    &[
        (0, Ink::Black, Ink::Default, true),
        (10, Ink::Magenta, Ink::Default, false),
        (20, Ink::Default, Ink::Default, false),
        (52, Ink::Magenta, Ink::Default, false),
        (53, Ink::Default, Ink::Default, false),
        (67, Ink::Magenta, Ink::Default, false),
    ],
    &[(10, Ink::Magenta, Ink::Default, false)],
];

/// The original BitchX ASCII "acidjazz" logo (`art.c`, ASCII build,
/// case 2). BSD-3-Clause, (c) 1990-2012 the BitchX authors.
pub(crate) const BANNER_ACID: [&str; 19] = [
    "                                                                   ,",
    "                                           .                     ,$",
    "                 .                                              ,$'",
    "                                           .        .          ,$'",
    "                 :      ,g$p,              .         $,       ,$'",
    "               y&$       `\"` .,.           $&y       `$,     ,$'",
    "               $$$     o oooy$$$yoo o      $$$        `$,   ,$' -acidjazz",
    "         .     $$$%%yyyp, gyp`$$$'gyyyyyyp, $$$yyyyp,   `$, ,$'     .",
    "       . yxxxx $$$\"`\"$$$ $$$ $$$ $y$\"`\"$$$ $$$\"`\"$$$ xxx`$,$'xxxxxxy .",
    "         $     $$7   l$$ $$$ $$$ $$7   \"\"\" $$7   ly$     .$'       $",
    "         $     $$b   dy$ $$$ $y$ $$b   $$$ $$b   d$$    ,$`$,      $",
    "       . $xxxx $$$uuu$$$ $$$ $$$ $$$uuu$$$ $$$   $$$ x ,$'x`$, xxxx$ .",
    "         .           \"\"\" \"\"\" \"\"\"       \"\"\"       \"\"\"  ,$'   `$,    .",
    "           b i t c h    -      x                     ,$'     `$,",
    "                                                     $'       `$,",
    "                                                    '          `$,",
    "                                                                `$,",
    "                                                                 `$",
    "                                                                   `",
];

/// Can this terminal render UTF-8 art (`TERM` + locale)?
///
/// `TERM` decides first: `dumb` and the legacy `vt1xx`/`ansi` names mean no,
/// names carrying `utf`/`256`/`color` mean yes. For everything else the
/// locale decides (`LC_ALL`/`LC_CTYPE`/`LANG` containing `UTF-8`).
pub(crate) fn utf8_terminal() -> bool {
    let term = std::env::var("TERM").unwrap_or_default();
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok())
        .unwrap_or_default();
    utf8_supported(&term, &locale)
}

/// The pure decision behind [`utf8_terminal`], so it can be tested.
pub(crate) fn utf8_supported(term: &str, locale: &str) -> bool {
    let term = term.to_ascii_lowercase();
    if !term.is_empty() {
        if term.contains("utf") || term.contains("256") || term.contains("color") {
            return true;
        }
        if term == "dumb"
            || term == "ansi"
            || term.starts_with("vt1")
            || term.starts_with("vt2")
            || term.starts_with("vt3")
            || term.contains("ascii")
        {
            return false;
        }
        // Modern terminals that are UTF-8 by default.
        if [
            "xterm",
            "screen",
            "tmux",
            "rxvt",
            "kitty",
            "alacritty",
            "wezterm",
            "foot",
            "ghostty",
            "konsole",
            "gnome",
            "iterm",
            "vte",
            "st-",
        ]
        .iter()
        .any(|k| term.contains(k))
        {
            return true;
        }
    }
    let loc = locale.to_ascii_uppercase();
    loc.contains("UTF-8") || loc.contains("UTF8")
}

/// Which splash logo the status window shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Logo {
    /// The original BitchX 1.3 art, decoded from CP437 to Unicode (78 cols).
    Original,
    /// The original BitchX ASCII "acidjazz" art (78 cols).
    Acid,
    /// No art at all: the terminal is too small for the splash. The
    /// `[ iamBitchX-ver]` credit line is still shown, it carries the version.
    None,
}

impl Logo {
    /// Pick the logo that fits this terminal.
    ///
    /// The original art wants 78 columns and about 36 rows; without that
    /// room no logo is drawn at all.
    pub(crate) fn pick(utf8: bool, width: u16, height: u16) -> Logo {
        let room = width >= 78 && height >= 36;
        match (utf8, room) {
            (_, false) => Logo::None,
            (true, true) => Logo::Original,
            (false, true) => Logo::Acid,
        }
    }

    /// The logo's lines, ready for the startup window.
    pub(crate) fn lines(self, ver: &str) -> Vec<Line> {
        let slot = format!("{ver:>15}");
        match self {
            Logo::Original => {
                // The original art, in the original's own colours: the text
                // rows plus the ink changes (column, fg, bg, bold).
                BANNER_ORIGINAL
                    .iter()
                    .zip(BANNER_ORIGINAL_INK.iter())
                    .map(|(text, ink)| art_row(text, ink, &slot))
                    .collect()
            },
            Logo::Acid => BANNER_ACID.iter().map(|l| Line::Logo(format!("    {l}"))).collect(),
            Logo::None => Vec::new(),
        }
    }

    /// How the startup header names the logo.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Logo::Original => "original bitchx logo",
            Logo::Acid => "acidjazz logo",
            Logo::None => "no logo",
        }
    }
}

/// One row of the splash art, in the original's colours: the text is cut
/// into runs at every ink change, and the version slot is filled in.
fn art_row(text: &str, ink: &[(u8, Ink, Ink, bool)], slot: &str) -> Line {
    let chars: Vec<char> = text.chars().collect();
    let mut runs: Vec<Run> = Vec::new();
    let mut cur = (Ink::Default, Ink::Default, false);
    let mut start = 0usize;
    for (at, fg, bg, bold) in ink {
        let upto = (*at as usize).min(chars.len());
        if upto > start {
            runs.push(Run {
                text: chars[start..upto].iter().collect(),
                fg: cur.0,
                bg: cur.1,
                bold: cur.2,
            });
        }
        start = upto;
        cur = (*fg, *bg, *bold);
    }
    if start < chars.len() {
        runs.push(Run {
            text: chars[start..].iter().collect(),
            fg: cur.0,
            bg: cur.1,
            bold: cur.2,
        });
    }
    Line::LogoRuns(
        runs.into_iter()
            .map(|r| Run { text: r.text.replace("%15s", slot), ..r })
            .collect(),
    )
}

/// The startup header line: terminal type and which logo is drawn.
pub(super) fn header_line(logo: Logo) -> Line {
    let term = std::env::var("TERM").unwrap_or_else(|_| "unknown".into());
    Line::Raw(format!("Using terminal type [{term}] ({})", logo.label()))
}

/// The startup lines of window 1, as in recon captures 00/01: version
/// header, the iamb credit line, logo with its `[ version]` credit, then
/// ` iamBitchX:` lines.
/// Where the logo starts in [`startup_lines_for`]: after the four header
/// lines, a blank one and the iamb credit line.
pub(crate) const LOGO_START: usize = 6;

pub(crate) fn startup_lines_for(nick: &str, logo: Logo) -> Vec<Line> {
    let ver = env!("CARGO_PKG_VERSION");
    let sha = option_env!("VERGEN_GIT_SHA").unwrap_or("unknown");
    let mut v = vec![
        Line::Raw("iamBitchX - Matrix client in the style of BitchX by panasync".into()),
        Line::Raw(format!("Version (iambitchx-{ver}) -- Commit ({sha}).")),
        Line::Raw(format!("Process [{}]", std::process::id())),
        header_line(logo),
        Line::Raw(String::new()),
    ];
    v.push(Line::Raw("    based on iamb - https://iamb.chat - by Ulyssa".into()));
    v.extend(logo.lines(ver));
    v.push(Line::Logo(format!("    [ iamBitchX-{ver}]           Matrix edition")));
    v.push(Line::Raw(String::new()));
    v.push(Line::Error(format!("Auto Response is set to - {nick}")));
    v.push(Line::Error("Type /help for help".into()));
    v
}
