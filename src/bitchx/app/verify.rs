//! Key verification (SAS emoji flows) and the room-encryption y/n
//! confirmation, driven by `/verify` and `/encrypt`.

use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

use crate::bitchx::app::{App, Confirm, PendingVerify};
use crate::bitchx::commands::VerifyCmd;
use crate::bitchx::e2ee::{encrypt_prompt, sas_lines};
use crate::bitchx::model::{Effect, Line, ReqDevice, VerifyAction, VerifyUpdate};

impl App {
    /// `/encrypt`: ask first; enabling encryption can never be undone.
    pub(super) fn encrypt(&mut self) -> Vec<Effect> {
        let Some(room_id) = self.current_room() else {
            return vec![];
        };
        // B1: the spoof-safe channel label plus the room ID, never the raw name.
        let name = self.cur().channel().unwrap_or(&self.cur().room.name).to_string();
        if self.cur().room.encrypted {
            self.info(format!("{name} [room id {room_id}] is already encrypted (+e)"));
            return vec![];
        }
        self.info(encrypt_prompt(&name, room_id.as_str()));
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
            Confirm::Create(alias) if yes => {
                self.info(format!("Creating channel #{alias}:{}...", self.me.server_name()));
                vec![Effect::CreateRoom { alias, public: false, encrypt: true }]
            },
            Confirm::Create(alias) => {
                self.info(format!("Not creating #{alias}"));
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

    /// `/verify start <user> [device|flow]` (E5): never guesses a flow. No
    /// pending flow: a new request (to `device` if given). Otherwise the arg
    /// selects a flow by id (prefix) or device; a device with no flow gets a
    /// new request; no arg goes through `pick_flow`, refusing when several.
    fn start_cmd(&mut self, user: OwnedUserId, device: Option<String>) -> Vec<Effect> {
        let new = |user, device| {
            vec![Effect::Verify {
                user,
                flow_id: None,
                action: VerifyAction::Start { device },
            }]
        };
        if self.flows_of(&user).is_empty() {
            return new(user, device);
        }
        let Some(arg) = device else {
            let Some(v) = self.pick_flow(&user, None) else {
                return vec![];
            };
            let action = VerifyAction::Start { device: None };
            return vec![Effect::Verify { user, flow_id: Some(v.flow_id), action }];
        };
        let by_flow = self.flows_of(&user).iter().any(|v| v.flow_id.starts_with(arg.as_str()));
        let by_dev = self.flows_of(&user).iter().any(|v| v.device.as_deref() == Some(&*arg));
        if by_flow && !by_dev {
            let Some(v) = self.pick_flow(&user, Some(&arg)) else {
                return vec![];
            };
            let action = VerifyAction::Start { device: None };
            return vec![Effect::Verify { user, flow_id: Some(v.flow_id), action }];
        }
        let flows: Vec<String> = self
            .flows_of(&user)
            .iter()
            .filter(|v| v.device.as_deref() == Some(&*arg))
            .map(|v| v.flow_id.clone())
            .collect();
        match flows.as_slice() {
            [] => new(user, Some(arg)),
            [f] => {
                let action = VerifyAction::Start { device: Some(arg) };
                vec![Effect::Verify { user, flow_id: Some(f.clone()), action }]
            },
            many => {
                self.error(format!(
                    "{user} has {} pending verifications with device {arg}: {} -- give the flow id",
                    many.len(),
                    many.join(", ")
                ));
                vec![]
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
        if let VerifyAction::Start { device } = action {
            return self.start_cmd(user, device);
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
            VerifyUpdate::Requested { flow_id, device, room_id, we_started, identity } => {
                // E3: the SDK keeps the first request of a reused flow id;
                // a later one must not replace the device shown for it.
                if self.verifications.iter().any(|v| v.flow_id == flow_id) {
                    let line = Line::Error(format!(
                        "Ignored a repeated verification request from {user} for flow {flow_id}"
                    ));
                    return self.verify_lines(room_id, vec![line]);
                }
                let mut dev = device.as_ref().map(req_device_label).unwrap_or_default();
                let subject = verify_subject(device.as_ref(), identity);
                if identity {
                    dev.push_str(" [IDENTITY: verifies ALL their devices]");
                }
                let device = match device {
                    Some(ReqDevice::Known { id, .. }) => Some(id),
                    _ => None,
                };
                let mut lines = vec![Line::Info(if we_started {
                    format!("Verification request sent to {user}{dev}, waiting for them to accept")
                } else {
                    format!(
                        "Verification request from {user}{dev} -- /verify accept {user} or /verify cancel {user}"
                    )
                })];
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
                    subject,
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
            VerifyUpdate::SasOffered => {
                set_state(self, "emoji offered");
                vec![Line::Info(format!(
                    "{user} started emoji verification -- /verify accept {user} to compare emoji, or /verify cancel {user}"
                ))]
            },
            VerifyUpdate::Emoji(emojis) => {
                set_state(self, "emoji");
                let subject = match self.flows_of(user).as_slice() {
                    [v] => v.subject.clone(),
                    _ => "which flow is unknown".to_owned(),
                };
                let mut lines = sas_lines(user.as_str(), &subject, &emojis);
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
                let mut subject = None;
                if n == 1 {
                    let i = self.verifications.iter().position(|v| v.user == *user);
                    subject = i.map(|i| self.verifications.remove(i).subject);
                } else {
                    // The flow we confirmed (never an ambiguous one) finished.
                    let done = self
                        .verifications
                        .iter()
                        .position(|v| v.user == *user && v.state == "confirmed");
                    if let Some(i) = done {
                        subject = Some(self.verifications.remove(i).subject);
                    }
                }
                // E4: say what was verified -- one device or the whole identity.
                vec![Line::Info(match subject {
                    Some(s) => format!("{user} is now verified ({s})"),
                    None => format!("{user} is now verified"),
                })]
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

/// E4: what a flow verifies, for the SAS header and the done line.
fn verify_subject(device: Option<&ReqDevice>, identity: bool) -> String {
    let s = crate::bitchx::format::sanitize;
    if identity {
        return "their cross-signing identity: ALL their devices".to_owned();
    }
    match device {
        Some(ReqDevice::Known { id, .. }) => format!("device {}", s(id)),
        Some(ReqDevice::Unknown(c)) => format!("UNKNOWN device \"{}\"", s(c)),
        None => "device unknown".to_owned(),
    }
}

/// E3: label a request's device by where it came from. A known device shows
/// the id and ed25519 key from the SDK's device list (key in groups of 4);
/// an unknown one is the sender's unverified claim.
fn req_device_label(d: &ReqDevice) -> String {
    let s = crate::bitchx::format::sanitize;
    match d {
        ReqDevice::Known { id, ed25519 } => {
            let key = ed25519.as_deref().map_or_else(
                || "no ed25519 key".to_owned(),
                |k| {
                    let k = s(k);
                    let groups: Vec<String> = k
                        .chars()
                        .collect::<Vec<_>>()
                        .chunks(4)
                        .map(|c| c.iter().collect())
                        .collect();
                    format!("ed25519 {}", groups.join(" "))
                },
            );
            format!(" [device {} ({key}), from their device list]", s(id))
        },
        ReqDevice::Unknown(claimed) => format!(
            " [UNKNOWN device \"{}\" -- not in their device list, sender's claim only]",
            s(claimed)
        ),
    }
}
