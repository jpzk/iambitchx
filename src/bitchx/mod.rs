//! # The BitchX client UI
//!
//! A from-scratch BitchX 1.x clone: numbered windows, two blue status lines,
//! an IRC input line. See `docs/v2-design.md` and `docs/bitchx-formats.md`.

pub(crate) mod app;
pub(crate) mod channels;
pub(crate) mod commands;
pub(crate) mod e2ee;
pub(crate) mod format;
pub(crate) mod input;
pub(crate) mod mention;
pub(crate) mod model;
pub(crate) mod status;
pub(crate) mod text;
pub(crate) mod ui;
pub(crate) mod vars;
