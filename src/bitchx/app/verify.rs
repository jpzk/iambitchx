//! Key verification (SAS emoji flows) and the room-encryption y/n
//! confirmation, driven by `/verify` and `/encrypt`.

use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

use crate::bitchx::app::{App, Confirm, PendingVerify};
use crate::bitchx::commands::VerifyCmd;
use crate::bitchx::e2ee::{encrypt_prompt, sas_lines};
use crate::bitchx::model::{Effect, Line, VerifyAction, VerifyUpdate};

impl App {
    /// `/encrypt`: ask first; enabling encryption can never be undone.
    pub(super) fn encrypt(&mut self) -> Vec<Effect> {
        let Some(room_id) = self.current_room() else {
            return vec![];
        };
        if self.cur().room.encrypted {
            self.info(format!("{} is already encrypted (+e)", self.cur().room.name));
            return vec![];
        }
        let name = self.cur().room.name.clone();
        self.info(encrypt_prompt(&name));
        self.confirm = Some(Confirm::Encrypt(room_id));
        vec![]
    }

    /// The line typed after a y/n question.
    pub(super) fn answer(&mut self, confirm: Confirm, line: &str) -> Vec<Effect> {
        let yes = matches!(line.trim().to_lowercase().as_str(), "y" | "yes");
        match confirm {
            Confirm::Encrypt(room_id) if yes => {
                self.info("Enabling end-to-end encryption (megolm)...");
                vec![Effect::EnableEncryption(room_id)]
            },
            Confirm::Encrypt(_) => {
                self.info("Encryption not enabled");
                vec![]
            },
        }
    }

    /// The pending flows with `user` (F44: one entry per flow, not per user).
    pub(super) fn flows_of(&self, user: &OwnedUserId) -> Vec<&PendingVerify> {
        self.verifications.iter().filter(|v| &v.user == user).collect()
    }

    /// The one flow a `/verify <sub> <user> [flow]` means. `flow` is a flow id
    /// or a unique prefix of one. Several flows and no `flow`: refused, listing
    /// them, so a confirm can never land on a flow whose emoji were not shown.
    pub(super) fn pick_flow(
        &mut self,
        user: &OwnedUserId,
        flow: Option<&str>,
    ) -> Option<PendingVerify> {
        let flows = self.flows_of(user);
        let picked: Vec<&PendingVerify> = match flow {
            Some(f) => {
                let exact: Vec<_> = flows.iter().copied().filter(|v| v.flow_id == f).collect();
                if exact.is_empty() {
                    flows.iter().copied().filter(|v| v.flow_id.starts_with(f)).collect()
                } else {
                    exact
                }
            },
            None => flows,
        };
        match picked.as_slice() {
            [one] => Some((*one).clone()),
            [] => {
                let what = flow.map(|f| format!(" (flow {f})")).unwrap_or_default();
                self.error(format!("No pending verification with {user}{what} -- see /verify"));
                None
            },
            many => {
                let list: Vec<String> = many
                    .iter()
                    .map(|v| {
                        let dev =
                            v.device.as_deref().map(|d| format!(" [{d}]")).unwrap_or_default();
                        format!("{}{dev}", v.flow_id)
                    })
                    .collect();
                self.error(format!(
                    "{user} has {} pending verifications: {} -- add the flow id",
                    many.len(),
                    list.join(", ")
                ));
                None
            },
        }
    }

    pub(super) fn verify_cmd(&mut self, cmd: VerifyCmd) -> Vec<Effect> {
        let (target, flow, action) = match cmd {
            VerifyCmd::List => {
                if self.verifications.is_empty() {
                    self.info("No pending verifications");
                }
                let rows: Vec<String> = self
                    .verifications
                    .iter()
                    .map(|v| {
                        let dir = if v.we_started { "to" } else { "from" };
                        let dev =
                            v.device.as_deref().map(|d| format!(" [{d}]")).unwrap_or_default();
                        let amb = if v.ambiguous { ", emoji ambiguous" } else { "" };
                        format!(
                            "Verification {dir} {}{dev}: {}{amb} (flow {})",
                            v.user, v.state, v.flow_id
                        )
                    })
                    .collect();
                for r in rows {
                    self.info(r);
                }
                return vec![];
            },
            VerifyCmd::Accept { user, flow } => (user, flow, VerifyAction::Accept),
            VerifyCmd::Start { user, device } => (user, None, VerifyAction::Start { device }),
            VerifyCmd::Confirm { user, flow } => (user, flow, VerifyAction::Confirm),
            VerifyCmd::Mismatch { user, flow } => (user, flow, VerifyAction::Mismatch),
            VerifyCmd::Cancel { user, flow } => (user, flow, VerifyAction::Cancel),
        };
        let Some(user) = self.user_cmd(&target) else {
            return vec![];
        };
        if let VerifyAction::Start { .. } = action {
            let flow_id = self.flows_of(&user).last().map(|v| v.flow_id.clone());
            return vec![Effect::Verify { user, flow_id, action }];
        }
        let Some(v) = self.pick_flow(&user, flow.as_deref()) else {
            return vec![];
        };
        // The emoji on screen cannot be tied to this flow (several flows with
        // this user reached SAS): never confirm it, start over.
        if v.ambiguous && action == VerifyAction::Confirm {
            self.error(format!(
                "Emoji shown for {user} may belong to another flow -- /verify cancel {user} {} and start again",
                v.flow_id
            ));
            return vec![];
        }
        if matches!(action, VerifyAction::Cancel | VerifyAction::Mismatch)
            && let Some(p) = self.verifications.iter_mut().find(|p| p.flow_id == v.flow_id)
        {
            // the Matrix side's `Cancelled` (no flow id) then removes this one
            p.state = "cancelled";
        }
        vec![Effect::Verify { user, flow_id: Some(v.flow_id), action }]
    }

    /// Print verification lines in the room of an in-room request (else the
    /// status window), and in the current window too so they are not missed.
    pub(super) fn verify_lines(&mut self, room_id: Option<OwnedRoomId>, lines: Vec<Line>) {
        let idx = room_id.and_then(|r| self.window_of(&r)).unwrap_or(0);
        for l in &lines {
            self.windows[idx].push(None, l.clone());
        }
        if idx != self.current {
            self.windows[idx].unread = true;
            for l in lines {
                self.cur_mut().push(None, l);
            }
        }
    }

    /// F44: the Matrix side reports progress per user (no flow id yet), so an
    /// update is tied to a flow only while that user has exactly one. With
    /// several flows, SAS emoji mark them all `ambiguous` (confirm refused).
    pub(super) fn verify_update(&mut self, user: &OwnedUserId, update: VerifyUpdate) {
        let n = self.flows_of(user).len();
        let room_id = self.flows_of(user).last().and_then(|v| v.room_id.clone());
        let set_state = |app: &mut App, state| {
            if n == 1
                && let Some(v) = app.verifications.iter_mut().find(|v| v.user == *user)
            {
                v.state = state;
            }
        };
        let lines = match update {
            VerifyUpdate::Requested { flow_id, device, room_id, we_started } => {
                let dev = device.as_deref().map(|d| format!(" [{d}]")).unwrap_or_default();
                let mut lines = vec![Line::Info(if we_started {
                    format!("Verification request sent to {user}{dev}, waiting for them to accept")
                } else {
                    format!(
                        "Verification request from {user}{dev} -- /verify accept {user} or /verify cancel {user}"
                    )
                })];
                self.verifications.retain(|v| v.flow_id != flow_id);
                if !self.flows_of(user).is_empty() {
                    lines.push(Line::Error(format!(
                        "{user} has another verification pending -- commands now need the flow id ({flow_id}), see /verify"
                    )));
                }
                self.verifications.push(PendingVerify {
                    user: user.clone(),
                    flow_id,
                    device,
                    room_id: room_id.clone(),
                    we_started,
                    state: "requested",
                    ambiguous: false,
                });
                return self.verify_lines(room_id, lines);
            },
            VerifyUpdate::Ready => {
                set_state(self, "ready");
                vec![Line::Info(format!(
                    "Verification with {user} is ready, starting emoji (SAS)"
                ))]
            },
            VerifyUpdate::Emoji(emojis) => {
                set_state(self, "emoji");
                let mut lines = sas_lines(user.as_str(), &emojis);
                if n > 1 {
                    for v in self.verifications.iter_mut().filter(|v| v.user == *user) {
                        v.ambiguous = true;
                    }
                    lines.push(Line::Error(format!(
                        "{n} verifications with {user} are pending: these emoji cannot be tied to one of them, confirm is refused -- cancel them and start again"
                    )));
                }
                lines
            },
            VerifyUpdate::Confirmed => {
                set_state(self, "confirmed");
                vec![Line::Info(format!(
                    "Waiting for {user} to confirm the emoji..."
                ))]
            },
            VerifyUpdate::Done => {
                if n == 1 {
                    self.verifications.retain(|v| v.user != *user);
                } else {
                    // The flow we confirmed (never an ambiguous one) finished.
                    let done = self
                        .verifications
                        .iter()
                        .position(|v| v.user == *user && v.state == "confirmed");
                    if let Some(i) = done {
                        self.verifications.remove(i);
                    }
                }
                vec![Line::Info(format!("{user} is now verified"))]
            },
            VerifyUpdate::Cancelled(reason) => {
                let mut lines = vec![Line::Error(format!(
                    "Verification with {user} cancelled: {reason}"
                ))];
                let ours = self
                    .verifications
                    .iter()
                    .position(|v| v.user == *user && v.state == "cancelled");
                if n == 1 {
                    self.verifications.retain(|v| v.user != *user);
                } else if let Some(i) = ours {
                    self.verifications.remove(i);
                } else if n > 1 {
                    // Which flow ended is unknown: keep them, see /verify.
                    for v in self.verifications.iter_mut().filter(|v| v.user == *user) {
                        v.ambiguous = true;
                    }
                    lines.push(Line::Error(format!(
                        "One of {n} verifications with {user} ended -- /verify cancel {user} <flow> the rest"
                    )));
                }
                lines
            },
        };
        self.verify_lines(room_id, lines);
    }
}
