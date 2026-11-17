use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    LineUp,
    LineDown,
    PageNext,
    PagePrev,
    ChapterNext,
    ChapterPrev,
    ToggleHelp,
    ToggleToc,
    ToggleViewMode,
    OpenSearch,
    Confirm,
    Quit,
    Resize(u16, u16),
}

pub fn translate(event: Event) -> Option<Action> {
    match event {
        Event::Resize(cols, rows) => Some(Action::Resize(cols, rows)),
        Event::Key(key) => translate_key(key),
        _ => None,
    }
}

fn translate_key(key: KeyEvent) -> Option<Action> {
    use KeyCode::*;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // On Windows the console reports both a key-down and a key-up record
    // per physical press, which crossterm surfaces as `Press` followed by
    // `Release` events. Acting on both would fire every action twice, so
    // only react to genuine presses. (Reader-only bindings are all
    // press-triggered; holding a key to repeat is handled by the OS's
    // autorepeat, which Windows reports with `key_down` set.)
    //
    // On Unix this is effectively a no-op for normal terminals: crossterm
    // parses ANSI sequences and always reports `KeyEventKind::Press`
    // there. The one caveat is the Kitty keyboard protocol: if it were
    // enabled, crossterm reports held-key autorepeat as
    // `KeyEventKind::Repeat` (and key releases as `Release`), both of
    // which this guard drops — so a held key would NOT auto-scroll. We
    // never enable the Kitty protocol (cleader has no reason to), so this
    // path is unreachable today; if it's ever enabled, Repeat would need
    // explicit handling to restore hold-to-scroll.
    if key.kind != KeyEventKind::Press {
        return None;
    }

    match (key.code, ctrl, shift) {
        // Ctrl+C quits (SIGINT analog). Ctrl+Q is XON in many terminals,
        // so plain 'q' only — never accept Ctrl+Q as Quit.
        (Char('c'), true, false) => Some(Action::Quit),
        (Char('q'), false, _) => Some(Action::Quit),
        (Esc, _, _) => Some(Action::Quit),

        (Up, _, _) | (Char('k'), false, false) => Some(Action::LineUp),
        (Down, _, _) | (Char('j'), false, false) => Some(Action::LineDown),

        (Left, _, _) | (Char('h'), false, false) => Some(Action::PagePrev),
        (PageUp, _, _) | (Char('b'), false, false) => Some(Action::PagePrev),

        (Right, _, _) | (Char('l'), false, false) => Some(Action::PageNext),
        (PageDown, _, _) | (Char(' '), false, false) => Some(Action::PageNext),

        (Char('n'), false, false) => Some(Action::ChapterNext),
        (Char('N'), false, _) => Some(Action::ChapterPrev),

        // ? maps to ToggleHelp regardless of the SHIFT modifier — US
        // layouts produce ? via Shift+/ (which arrives as Char('?')
        // with SHIFT set), but kitty/some terminals report it bare.
        (Char('?'), false, _) => Some(Action::ToggleHelp),

        (Char('t'), false, false) => Some(Action::ToggleToc),
        (Char('g'), false, false) => Some(Action::ToggleViewMode),
        (Char('/'), false, _) => Some(Action::OpenSearch),
        (Enter, _, _) => Some(Action::Confirm),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventKind, KeyEventState};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    fn key_with(code: KeyCode, mods: KeyModifiers) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    fn key_release(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        })
    }

    #[test]
    fn release_events_are_ignored() {
        // The Windows console (incl. the ConPTY used by Git Bash/mintty)
        // reports a key-up record for every key-down. crossterm surfaces
        // these as KeyEventKind::Release; they must not trigger actions,
        // or every physical press fires twice.
        for code in [
            KeyCode::Char('j'),
            KeyCode::Char('h'),
            KeyCode::Char('l'),
            KeyCode::Char(' '),
            KeyCode::Char('n'),
            KeyCode::Char('N'),
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::PageUp,
            KeyCode::PageDown,
        ] {
            assert_eq!(
                translate(key_release(code)),
                None,
                "Release event for {code:?} must not map to an action"
            );
        }
    }

    #[test]
    fn arrow_keys_map_correctly() {
        assert_eq!(translate(key(KeyCode::Up)), Some(Action::LineUp));
        assert_eq!(translate(key(KeyCode::Down)), Some(Action::LineDown));
        assert_eq!(translate(key(KeyCode::Left)), Some(Action::PagePrev));
        assert_eq!(translate(key(KeyCode::Right)), Some(Action::PageNext));
    }

    #[test]
    fn vim_keys_map_correctly() {
        assert_eq!(translate(key(KeyCode::Char('k'))), Some(Action::LineUp));
        assert_eq!(translate(key(KeyCode::Char('j'))), Some(Action::LineDown));
        assert_eq!(translate(key(KeyCode::Char('h'))), Some(Action::PagePrev));
        assert_eq!(translate(key(KeyCode::Char('l'))), Some(Action::PageNext));
    }

    #[test]
    fn page_aliases_map_correctly() {
        assert_eq!(translate(key(KeyCode::PageUp)), Some(Action::PagePrev));
        assert_eq!(translate(key(KeyCode::PageDown)), Some(Action::PageNext));
        assert_eq!(translate(key(KeyCode::Char('b'))), Some(Action::PagePrev));
        assert_eq!(translate(key(KeyCode::Char(' '))), Some(Action::PageNext));
    }

    #[test]
    fn chapter_keys_map_correctly() {
        assert_eq!(translate(key(KeyCode::Char('n'))), Some(Action::ChapterNext));
        let shift_n = key_with(KeyCode::Char('N'), KeyModifiers::SHIFT);
        assert_eq!(translate(shift_n), Some(Action::ChapterPrev));
    }

    #[test]
    fn quit_keys_map_correctly() {
        assert_eq!(translate(key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(translate(key(KeyCode::Esc)), Some(Action::Quit));
        let ctrl_c = key_with(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(translate(ctrl_c), Some(Action::Quit));
    }

    #[test]
    fn resize_event_maps_correctly() {
        assert_eq!(translate(Event::Resize(120, 40)), Some(Action::Resize(120, 40)));
    }

    #[test]
    fn question_mark_toggles_help() {
        // ? on a US layout arrives as Char('?') with SHIFT modifier.
        let q = key_with(KeyCode::Char('?'), KeyModifiers::SHIFT);
        assert_eq!(translate(q), Some(Action::ToggleHelp));
        // ? without shift (kitty/some terminals report it bare) also works.
        assert_eq!(
            translate(key(KeyCode::Char('?'))),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn t_toggles_toc() {
        assert_eq!(translate(key(KeyCode::Char('t'))), Some(Action::ToggleToc));
    }

    #[test]
    fn enter_is_confirm() {
        assert_eq!(translate(key(KeyCode::Enter)), Some(Action::Confirm));
    }

    #[test]
    fn unknown_key_returns_none() {
        assert_eq!(translate(key(KeyCode::Char('x'))), None);
        assert_eq!(translate(key(KeyCode::F(1))), None);
        assert_eq!(translate(key(KeyCode::Tab)), None);
    }

    #[test]
    fn g_toggles_view_mode() {
        assert_eq!(
            translate(key(KeyCode::Char('g'))),
            Some(Action::ToggleViewMode)
        );
    }

    #[test]
    fn slash_opens_search() {
        assert_eq!(
            translate(key(KeyCode::Char('/'))),
            Some(Action::OpenSearch)
        );
        // Some terminals report bare '/' with SHIFT set; the binding
        // accepts both (mirrors the ? / ToggleHelp pattern).
        let shift_slash = key_with(KeyCode::Char('/'), KeyModifiers::SHIFT);
        assert_eq!(translate(shift_slash), Some(Action::OpenSearch));
    }
}
