//! Reactions (`/react`), typing notices (`SEND_TYPING`) and read receipts
//! (`SEND_RECEIPTS`): the Matrix chatter IRC never had.

use crate::bitchx::app::{App, TYPING_RESEND_MS};
use crate::bitchx::model::Effect;

impl App {
    /// `/react <key> [N]`: react to the last (N = 1) or Nth-last message of
    /// the current window.
    pub(super) fn react(&mut self, key: String, nth: usize) -> Vec<Effect> {
        let Some(room_id) = self.current_room() else {
            return vec![];
        };
        let msgs = &self.cur().msgs;
        let Some((event_id, _)) = nth.checked_sub(1).and_then(|i| msgs.iter().rev().nth(i)) else {
            let have = msgs.len();
            self.error(format!("No message {nth} to react to ({have} in this window)"));
            return vec![];
        };
        let event_id = event_id.clone();
        vec![Effect::React { room_id, event_id, key }]
    }

    /// A read receipt for the newest event of window `idx`, if it is the
    /// active window, `SEND_RECEIPTS` is on and that event was not acked.
    pub(crate) fn read_receipt(&mut self, idx: usize) -> Vec<Effect> {
        if idx != self.current || !self.vars.send_receipts {
            return vec![];
        }
        let win = &mut self.windows[idx];
        let (Some(room_id), Some(event_id)) = (win.room_id().cloned(), win.last_event.clone())
        else {
            return vec![];
        };
        if win.receipt_sent.as_ref() == Some(&event_id) {
            return vec![];
        }
        win.receipt_sent = Some(event_id.clone());
        vec![Effect::Receipt { room_id, event_id }]
    }

    /// After an edit of the input line: start, refresh (every
    /// [`TYPING_RESEND_MS`]) or stop the typing notice. We are typing while
    /// the line has text that is not a `/command`, in a room window, with
    /// `SEND_TYPING` on; sending or clearing the line stops it, and so does
    /// switching windows (the new room gets its own notice).
    pub(crate) fn typing_fx(&mut self) -> Vec<Effect> {
        let text = self.input.text();
        let command = text.starts_with('/') && !text.starts_with("//");
        let typing =
            self.vars.send_typing && self.confirm.is_none() && !text.trim().is_empty() && !command;
        let target = self.cur().room_id().cloned().filter(|_| typing);
        let now = self.now();
        let mut fx = vec![];
        if let Some((room, since)) = self.typing_room.take() {
            if Some(&room) == target.as_ref() {
                if now - since >= TYPING_RESEND_MS {
                    fx.push(Effect::Typing { room_id: room.clone(), typing: true });
                    self.typing_room = Some((room, now));
                } else {
                    self.typing_room = Some((room, since));
                }
                return fx;
            }
            fx.push(Effect::Typing { room_id: room, typing: false });
        }
        if let Some(room) = target {
            fx.push(Effect::Typing { room_id: room.clone(), typing: true });
            self.typing_room = Some((room, now));
        }
        fx
    }
}
