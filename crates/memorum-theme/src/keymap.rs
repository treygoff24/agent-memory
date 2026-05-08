use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Enter,
    OpenPalette,
    OpenSearch,
    OpenHelp,
    NextFilter,
    PrevFilter,
    AcceptItem,
    RejectItem,
    EditItem,
    ForgetItem,
    Confirm,
    Correct,
    Skip,
    PauseSession,
    Quit,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyChord {
    pub key: KeyCode,
    #[serde(default)]
    pub mods: KeyModifiers,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyCode {
    Char(char),
    Enter,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Tab,
    BackTab,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keymap {
    pub bindings: HashMap<KeyChord, Action>,
}

impl Keymap {
    pub fn vim_arrows() -> Self {
        let mut bindings = HashMap::new();
        for (key, action) in [
            (KeyCode::Char('j'), Action::MoveDown),
            (KeyCode::Down, Action::MoveDown),
            (KeyCode::Char('k'), Action::MoveUp),
            (KeyCode::Up, Action::MoveUp),
            (KeyCode::Char('h'), Action::MoveLeft),
            (KeyCode::Left, Action::MoveLeft),
            (KeyCode::Char('l'), Action::MoveRight),
            (KeyCode::Right, Action::MoveRight),
            (KeyCode::Enter, Action::Enter),
            (KeyCode::Char('p'), Action::OpenPalette),
            (KeyCode::Char('/'), Action::OpenSearch),
            (KeyCode::Char('?'), Action::OpenHelp),
            (KeyCode::Char('a'), Action::AcceptItem),
            (KeyCode::Char('r'), Action::RejectItem),
            (KeyCode::Char('e'), Action::EditItem),
            (KeyCode::Char('f'), Action::ForgetItem),
            (KeyCode::Char('q'), Action::Quit),
        ] {
            bindings.insert(KeyChord { key, mods: KeyModifiers::default() }, action);
        }
        Self { bindings }
    }

    pub fn emacs() -> Self {
        let mut keymap = Self::vim_arrows();
        keymap.bindings.insert(
            KeyChord { key: KeyCode::Char('n'), mods: KeyModifiers { ctrl: true, alt: false, shift: false } },
            Action::MoveDown,
        );
        keymap.bindings.insert(
            KeyChord { key: KeyCode::Char('p'), mods: KeyModifiers { ctrl: true, alt: false, shift: false } },
            Action::MoveUp,
        );
        keymap
    }

    pub fn merge_user_overrides(&mut self, overrides: HashMap<KeyChord, Action>) {
        self.bindings.extend(overrides);
    }
}
