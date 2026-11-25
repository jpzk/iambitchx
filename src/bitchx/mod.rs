//! # BitchX-style frontend
//!
//! An optional, IRC-flavoured view over the same store the classic UI
//! uses. See `docs/bitchx-frontend.md` for the design.
use serde::{Deserialize, Serialize};

pub mod commands;
pub mod dispatch;
pub mod screen;
pub mod status;

/// Which user interface to draw.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Frontend {
    /// The classic modal, tabbed, windowed interface inherited from upstream.
    ///
    /// Configured as `"classic"`; the legacy value `"iamb"` is still accepted.
    #[default]
    #[serde(rename = "classic", alias = "iamb")]
    Iamb,

    /// A BitchX-style status bar + full-screen chat + input line.
    Bitchx,
}
