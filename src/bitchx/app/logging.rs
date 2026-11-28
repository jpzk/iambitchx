//! `/lastlog` and `/set LOG`: searching a window and logging windows to
//! files (see [`crate::bitchx::lastlog`], [`crate::bitchx::logfile`]).
use std::path::PathBuf;

use super::{App, STATUS_NAME};
use crate::bitchx::lastlog;
use crate::bitchx::logfile::{line_rows, log_path};
use crate::bitchx::model::Line;
use crate::bitchx::vars::expand_log_dir;

impl App {
    /// `/lastlog [-i] TEXT`: the matching lines of the current window.
    pub(super) fn lastlog(&mut self, pattern: &str, ignore_case: bool) {
        let win = self.cur();
        let chan = win.channel().unwrap_or("").to_string();
        let out = lastlog::search(&win.lines, &win.times, &chan, pattern, ignore_case);
        let win = self.cur_mut();
        for line in out {
            win.push(None, line);
        }
        win.scroll = 0;
    }

    /// `/lastlog -clear`: remove all `/lastlog` output from the current window.
    pub(super) fn lastlog_clear(&mut self) {
        let win = self.cur_mut();
        let (lines, times): (Vec<Line>, Vec<i64>) = std::mem::take(&mut win.lines)
            .into_iter()
            .zip(std::mem::take(&mut win.times))
            .filter(|(l, _)| !lastlog::is_output(l))
            .unzip();
        win.lines = lines;
        win.times = times;
        win.scroll = 0;
    }

    /// The directory logs go to: `LOG_DIR`, else the default; `None` when
    /// neither is usable.
    pub(crate) fn log_dir(&self) -> Option<PathBuf> {
        if self.vars.log_dir.is_empty() {
            self.log_default_dir.clone()
        } else {
            expand_log_dir(&self.vars.log_dir)
        }
    }

    /// `LOG` or `LOG_DIR` changed: close the open files so the next line
    /// opens them where they now belong.
    pub(super) fn log_settings_changed(&mut self) {
        let now = self.now();
        self.logger.close_all(now);
        if self.vars.log {
            match self.log_dir() {
                Some(dir) => self.info(format!("Logging to {}", dir.display())),
                None => self.error("LOG is on but no log directory is known: /set LOG_DIR path"),
            }
        }
    }

    /// Write the lines printed since the last call to the window log files
    /// (and forget them when `LOG` is off). Called once per event-loop turn.
    /// A file that cannot be written is reported once in the status window.
    pub(crate) fn flush_log(&mut self) {
        let now = self.now();
        let dir = self.vars.log.then(|| self.log_dir()).flatten();
        let server = self.me.server_name().to_string();
        let mut errors = vec![];
        for win in &mut self.windows {
            let queue = std::mem::take(&mut win.log_queue);
            let Some(dir) = dir.as_ref().filter(|_| !queue.is_empty()) else {
                continue;
            };
            let chan = win.channel().unwrap_or("").to_string();
            let name = if win.room_id().is_none() {
                STATUS_NAME
            } else {
                chan.as_str()
            };
            let path = log_path(dir, &server, name);
            for (ts, line) in queue {
                if let Err(e) = self.logger.write(&path, ts, now, &line_rows(&line, &chan)) {
                    errors.push(format!("Cannot write log {}: {e}", path.display()));
                    break;
                }
            }
        }
        for e in errors {
            self.windows[0].push(None, Line::Error(e));
        }
    }

    /// Quitting: flush and close every log file.
    pub(crate) fn close_logs(&mut self) {
        self.flush_log();
        let now = self.now();
        self.logger.close_all(now);
    }
}
