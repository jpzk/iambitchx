//! Debug-tap glue: pushes DEBUG settings to [`crate::debug_tap`] and
//! routes its lines into the status window behind a per-second flood guard.

use crate::bitchx::app::{App, DBG_PER_SEC};
use crate::bitchx::model::Line;

impl App {
    /// Push `DEBUG` / `DEBUG_FILTER` / `DEBUG_RAW` to the debug tap when they
    /// differ from what it has (cheap; runs before every Matrix event and
    /// after `/set`).
    pub(crate) fn apply_debug(&mut self) {
        let v = &self.vars;
        let want = (v.debug, v.debug_filter.clone(), v.debug_raw);
        if want != self.dbg_applied {
            crate::debug_tap::configure(want.0, &want.1, want.2);
            self.dbg_applied = want;
        }
    }

    /// A DBG line: always to the status window, never into a room window's
    /// scrollback (F-06); room-scoped lines are tagged with the room id. At
    /// most [`DBG_PER_SEC`] per second, then one `DBG (suppressed N)` when
    /// the next second starts.
    pub(super) fn debug_line(&mut self, line: crate::debug_tap::DbgLine) {
        let idx = 0;
        let sec = self.now().div_euclid(1000);
        let st = self.dbg_flood.entry(idx).or_insert((sec, 0, 0));
        let mut suppressed = 0;
        if st.0 != sec {
            suppressed = st.2;
            *st = (sec, 0, 0);
        }
        let show = st.1 < DBG_PER_SEC;
        if show {
            st.1 += 1;
        } else {
            st.2 += 1;
        }
        if suppressed > 0 {
            self.windows[idx].push(None, suppressed_line(suppressed));
        }
        if show {
            let cat = line.cat.name().to_string();
            let text = match line.room {
                Some(room) => format!("[{room}] {}", line.text),
                None => line.text,
            };
            self.windows[idx].push(None, Line::Debug { cat, text });
        }
    }

    /// Print the pending `DBG (suppressed N)` of windows whose second is over
    /// (from [`Self::tick`], so a burst's tail is not lost).
    pub(super) fn flush_debug(&mut self) {
        let sec = self.now().div_euclid(1000);
        let mut out = vec![];
        for (idx, st) in &mut self.dbg_flood {
            if st.0 != sec && st.2 > 0 {
                out.push((*idx, st.2));
                *st = (sec, 0, 0);
            }
        }
        for (idx, n) in out {
            if let Some(w) = self.windows.get_mut(idx) {
                w.push(None, suppressed_line(n));
            }
        }
    }
}

/// The flood guard's ` DBG (suppressed N)`.
fn suppressed_line(n: u32) -> Line {
    Line::Debug {
        cat: String::new(),
        text: format!("(suppressed {n})"),
    }
}
