//! # BitchX-style frontend
//!
//! An optional, IRC-flavoured view over the same store the normal iamb UI
//! uses. See `docs/bitchx-frontend.md` for the design.
use serde::{Deserialize, Serialize};

// Screen rendering lands in a follow-up change; until then only the tests
// exercise the status bar.
#[allow(dead_code)]
pub mod commands;
pub mod dispatch;
#[allow(dead_code)]
pub mod status;

/// Which user interface to draw.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Frontend {
    /// iamb's normal modal, tabbed, windowed interface.
    #[default]
    Iamb,

    /// A BitchX-style status bar + full-screen chat + input line.
    Bitchx,
}
