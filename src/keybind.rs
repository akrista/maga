use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// High-level semantic actions dispatched in the main workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    PageUp,
    PageDown,
    First,
    Last,
    ToggleSelected,
    ConfirmOrFold,
    FoldLeft,
    FoldRight,
    ToggleWorkspaceSection,
    SelectMcpsSection,
    SelectPluginsSection,
    StartFilter,
    AddForm,
    EditForm,
    OpenEditor,
    DeleteSelected,
    OpenConflicts,
    OpenOptions,
    ToggleHeroMascot,
    ToggleHideUi,
    RefreshFromDisk,
    ToggleAutoRefresh,
    SyncNow,
    ProbeSelected,
    ProbeAll,
    ToggleDetailsFocus,
    ToggleHelp,
    Quit,
    ConfirmDelete,
    CancelModal,
    ConflictChooseAgy,
    ConflictChooseClaude,
    ConflictSkip,
    FormNextField,
    FormPrevField,
    FormToggleTransport,
    FormSubmit,
    InputChar(char),
    InputBackspace,
}

pub fn map_workspace_key(key: KeyEvent) -> Option<Action> {
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => Some(Action::Quit),
        (KeyModifiers::NONE, KeyCode::Char('q')) => Some(Action::Quit),
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            Some(Action::Down)
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            Some(Action::Up)
        }
        (KeyModifiers::NONE, KeyCode::PageUp) | (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
            Some(Action::PageUp)
        }
        (KeyModifiers::NONE, KeyCode::PageDown) | (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
            Some(Action::PageDown)
        }
        (KeyModifiers::NONE, KeyCode::Home) | (KeyModifiers::NONE, KeyCode::Char('g')) => {
            Some(Action::First)
        }
        (KeyModifiers::NONE, KeyCode::End)
        | (KeyModifiers::SHIFT, KeyCode::Char('G'))
        | (KeyModifiers::NONE, KeyCode::Char('G')) => Some(Action::Last),
        (KeyModifiers::NONE, KeyCode::Char(' ')) => Some(Action::ToggleSelected),
        (KeyModifiers::NONE, KeyCode::Enter) => Some(Action::ConfirmOrFold),
        (KeyModifiers::NONE, KeyCode::Char('h')) | (KeyModifiers::NONE, KeyCode::Left) => {
            Some(Action::FoldLeft)
        }
        (KeyModifiers::NONE, KeyCode::Char('l')) | (KeyModifiers::NONE, KeyCode::Right) => {
            Some(Action::FoldRight)
        }
        (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            Some(Action::ToggleWorkspaceSection)
        }
        (KeyModifiers::NONE, KeyCode::Char('1')) => Some(Action::SelectMcpsSection),
        (KeyModifiers::NONE, KeyCode::Char('2')) => Some(Action::SelectPluginsSection),
        (KeyModifiers::NONE, KeyCode::Char('/')) => Some(Action::StartFilter),
        (KeyModifiers::NONE, KeyCode::Char('a')) => Some(Action::AddForm),
        (KeyModifiers::NONE, KeyCode::Char('e')) => Some(Action::EditForm),
        (KeyModifiers::SHIFT, KeyCode::Char('E')) | (KeyModifiers::NONE, KeyCode::Char('E')) => {
            Some(Action::OpenEditor)
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) => Some(Action::DeleteSelected),
        (KeyModifiers::NONE, KeyCode::Char('c')) => Some(Action::OpenConflicts),
        (KeyModifiers::NONE, KeyCode::Char('o')) => Some(Action::OpenOptions),
        (KeyModifiers::NONE, KeyCode::Char('m'))
        | (KeyModifiers::NONE, KeyCode::Char('M'))
        | (KeyModifiers::SHIFT, KeyCode::Char('M')) => Some(Action::ToggleHeroMascot),
        (KeyModifiers::NONE, KeyCode::Char('b'))
        | (KeyModifiers::NONE, KeyCode::Char('B'))
        | (KeyModifiers::SHIFT, KeyCode::Char('B')) => Some(Action::ToggleHideUi),
        (KeyModifiers::NONE, KeyCode::Char('r')) => Some(Action::RefreshFromDisk),
        (KeyModifiers::SHIFT, KeyCode::Char('R')) | (KeyModifiers::NONE, KeyCode::Char('R')) => {
            Some(Action::ToggleAutoRefresh)
        }
        (KeyModifiers::NONE, KeyCode::Char('s')) => Some(Action::SyncNow),
        (KeyModifiers::NONE, KeyCode::Char('p')) => Some(Action::ProbeSelected),
        (KeyModifiers::SHIFT, KeyCode::Char('P')) | (KeyModifiers::NONE, KeyCode::Char('P')) => {
            Some(Action::ProbeAll)
        }
        (KeyModifiers::NONE, KeyCode::Char('t')) => Some(Action::ToggleDetailsFocus),
        (KeyModifiers::NONE, KeyCode::Char('?')) => Some(Action::ToggleHelp),
        (KeyModifiers::NONE, KeyCode::Esc) => Some(Action::CancelModal),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_extended_navigation_keys() {
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
            Some(Action::PageUp)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            Some(Action::PageUp)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            Some(Action::PageDown)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Some(Action::PageDown)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)),
            Some(Action::First)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
            Some(Action::First)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
            Some(Action::Last)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)),
            Some(Action::Last)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
            Some(Action::RefreshFromDisk)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)),
            Some(Action::OpenOptions)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)),
            Some(Action::ToggleHeroMascot)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)),
            Some(Action::ToggleAutoRefresh)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
            Some(Action::SyncNow)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
            Some(Action::ProbeSelected)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT)),
            Some(Action::ProbeAll)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)),
            Some(Action::ToggleDetailsFocus)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::ToggleWorkspaceSection)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)),
            Some(Action::SelectMcpsSection)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            Some(Action::SelectPluginsSection)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::ConfirmOrFold)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)),
            Some(Action::FoldLeft)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            Some(Action::FoldLeft)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)),
            Some(Action::FoldRight)
        );
        assert_eq!(
            map_workspace_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            Some(Action::FoldRight)
        );
    }
}
