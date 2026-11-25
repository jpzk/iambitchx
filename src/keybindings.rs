//! # Default Keybindings
//!
//! The keybindings are set up here. We define some iamb-specific keybindings, but the default Vim
//! keys come from [modalkit::env::vim::keybindings].
use modalkit::actions::{EditAction, MacroAction};
use modalkit::env::CommonKeyClass;
use modalkit::env::vim::VimMode;
use modalkit::env::vim::keybindings::{InputStep, VimBindings};
use modalkit::keybindings::{EdgeEvent, EdgeRepeat, InputBindings};

use crate::base::{Keybindings, MATRIX_ID_WORD};
use crate::bitchx::Frontend;
use crate::config::Keys;
use crate::prelude::*;

pub type IambStep = InputStep<IambInfo>;

fn once(key: &TerminalKey) -> (EdgeRepeat, EdgeEvent<TerminalKey, CommonKeyClass>) {
    (EdgeRepeat::Once, EdgeEvent::Key(*key))
}

/// Initialize the default keybinding state.
pub fn setup_keybindings(tunables: &TunableValues) -> Keybindings {
    let mut ism = Keybindings::empty();

    let vim = VimBindings::default()
        .default_split(tunables.default_split.into())
        .submit_on_enter(!tunables.send_on_enter)
        .cursor_open(MATRIX_ID_WORD.clone());

    vim.setup(&mut ism);

    let ctrl_w = "<C-W>".parse::<TerminalKey>().unwrap();
    let ctrl_m = "<C-M>".parse::<TerminalKey>().unwrap();
    let ctrl_z = "<C-Z>".parse::<TerminalKey>().unwrap();
    let key_m_lc = "m".parse::<TerminalKey>().unwrap();
    let key_z_lc = "z".parse::<TerminalKey>().unwrap();
    let shift_enter = "<S-Enter>".parse::<TerminalKey>().unwrap();

    let cwz = vec![once(&ctrl_w), once(&key_z_lc)];
    let cwcz = vec![once(&ctrl_w), once(&ctrl_z)];
    let zoom = IambStep::new()
        .actions(vec![WindowAction::ZoomToggle.into()])
        .goto(VimMode::Normal);

    ism.add_mapping(VimMode::Normal, &cwz, &zoom);
    ism.add_mapping(VimMode::Visual, &cwz, &zoom);
    ism.add_mapping(VimMode::Normal, &cwcz, &zoom);
    ism.add_mapping(VimMode::Visual, &cwcz, &zoom);

    let cwm = vec![once(&ctrl_w), once(&key_m_lc)];
    let cwcm = vec![once(&ctrl_w), once(&ctrl_m)];
    let stoggle = IambStep::new()
        .actions(vec![IambAction::ToggleScrollbackFocus.into()])
        .goto(VimMode::Normal);
    ism.add_mapping(VimMode::Normal, &cwm, &stoggle);
    ism.add_mapping(VimMode::Visual, &cwm, &stoggle);
    ism.add_mapping(VimMode::Normal, &cwcm, &stoggle);
    ism.add_mapping(VimMode::Visual, &cwcm, &stoggle);

    let shift_enter = vec![once(&shift_enter)];
    let newline = IambStep::new().actions(vec![
        InsertTextAction::Type(Char::Single('\n').into(), MoveDir1D::Previous, 1.into()).into(),
    ]);
    ism.add_mapping(VimMode::Insert, &cwm, &newline);
    ism.add_mapping(VimMode::Insert, &shift_enter, &newline);

    if tunables.frontend == Frontend::Bitchx {
        setup_bitchx_keybindings(&mut ism);
    }

    ism
}

impl InputBindings<TerminalKey, IambStep> for ApplicationSettings {
    fn setup(&self, bindings: &mut Keybindings) {
        for (modes, keys) in &self.macros {
            for (Keys(input, _), Keys(_, run)) in keys {
                let act = MacroAction::Run(run.clone(), Count::Contextual);
                let step = IambStep::new().actions(vec![act.into()]);
                let input = input.iter().map(once).collect::<Vec<_>>();

                for mode in &modes.0 {
                    bindings.add_mapping(*mode, &input, &step);
                }
            }
        }
    }
}

/// The key fed to the key manager to (re-)enter Insert mode in the BitchX frontend.
///
/// This is Vim's plain `i`, so entering Insert mode behaves exactly like in iamb.
pub fn bitchx_insert_key() -> TerminalKey {
    "i".parse::<TerminalKey>().unwrap()
}

/// Action that clears the whole input line (the message bar).
pub fn bitchx_clear_line() -> ProgramAction {
    let op = EditAction::Delete(Specifier::Contextual, Specifier::Contextual);
    let target = EditTarget::Range(RangeType::Buffer, true, Count::Contextual);

    EditorAction::Edit(Specifier::Exact(op), target).into()
}

/// BitchX frontend: `Esc` in Insert mode abandons the current input line.
///
/// The step still drops to Normal mode, so that `Esc` keeps working in the
/// non-room windows (`:rooms`, `:members`, ...); the application puts the key
/// manager straight back into Insert mode whenever a joined room is focused
/// (see `Application::bitchx_wants_insert`). All other iamb bindings,
/// including `Ctrl-O` for a single Normal mode command, stay as they are.
fn setup_bitchx_keybindings(ism: &mut Keybindings) {
    let esc = vec![once(&"<Esc>".parse::<TerminalKey>().unwrap())];
    let clear = IambStep::new().actions(vec![bitchx_clear_line()]).goto(VimMode::Normal);

    ism.add_mapping(VimMode::Insert, &esc, &clear);
}

#[cfg(test)]
mod tests {
    use modalkit::editing::key::KeyManager;
    use modalkit::keybindings::BindingMachine;

    use super::*;
    use crate::tests::mock_tunables;

    fn manager(frontend: Frontend) -> KeyManager<TerminalKey, ProgramAction, RepeatType> {
        let mut tunables = mock_tunables();
        tunables.frontend = frontend;

        KeyManager::new(setup_keybindings(&tunables))
    }

    fn key(k: &str) -> TerminalKey {
        k.parse::<TerminalKey>().unwrap()
    }

    fn drain(
        bindings: &mut KeyManager<TerminalKey, ProgramAction, RepeatType>,
    ) -> Vec<ProgramAction> {
        std::iter::from_fn(|| bindings.pop()).map(|(act, _)| act).collect()
    }

    #[test]
    fn test_bitchx_force_insert_mode() {
        let mut bindings = manager(Frontend::Bitchx);
        assert_eq!(bindings.show_mode(), None);

        // What the application feeds the key manager while a room is focused.
        bindings.input_key(bitchx_insert_key());
        assert_eq!(bindings.show_mode().as_deref(), Some("-- INSERT --"));
        drain(&mut bindings);

        // Printable keys now go straight into the input line.
        bindings.input_key(key("x"));
        assert_eq!(bindings.show_mode().as_deref(), Some("-- INSERT --"));
        let acts = drain(&mut bindings);
        assert!(
            acts.iter()
                .any(|a| matches!(a, Action::Editor(EditorAction::InsertText(_)))),
            "{acts:?}"
        );
    }

    #[test]
    fn test_bitchx_esc_clears_line() {
        let mut bindings = manager(Frontend::Bitchx);
        bindings.input_key(bitchx_insert_key());
        drain(&mut bindings);

        bindings.input_key(key("<Esc>"));
        let acts = drain(&mut bindings);
        assert!(acts.contains(&bitchx_clear_line()), "{acts:?}");
    }

    #[test]
    fn test_iamb_esc_does_not_clear_line() {
        let mut bindings = manager(Frontend::Iamb);
        bindings.input_key(bitchx_insert_key());
        drain(&mut bindings);

        bindings.input_key(key("<Esc>"));
        assert_eq!(bindings.show_mode(), None);
        let acts = drain(&mut bindings);
        assert!(!acts.contains(&bitchx_clear_line()), "{acts:?}");
    }
}
