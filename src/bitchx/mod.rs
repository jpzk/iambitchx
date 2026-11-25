//! # BitchX-style frontend
//!
//! An optional, IRC-flavoured view over the same store the normal iamb UI
//! uses. See `docs/bitchx-frontend.md` for the design.
use serde::{Deserialize, Serialize};

// Not wired into the UI yet: dispatch (chat.rs) and screen rendering land in
// follow-up changes. Until then only the tests exercise these modules.
#[allow(dead_code)]
pub mod commands;
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
