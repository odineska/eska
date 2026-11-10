//! Inline confirmation keeps the full replacement preview visible in terminal scrollback.

use std::{io, time::Duration};

use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute, terminal,
};

use super::PromptError;

/// Accept only an explicit overwrite key, ignoring pasted text and repeated key events.
///
/// # Errors
/// Returns cancellation or terminal setup/input failure.
pub fn confirm_overwrite(interrupted: impl Fn() -> bool) -> Result<bool, PromptError> {
    terminal::enable_raw_mode().map_err(|_| PromptError::Io)?;
    let _guard = ConfirmationTerminal;
    execute!(io::stderr(), EnableBracketedPaste).map_err(|_| PromptError::Io)?;
    loop {
        if interrupted() {
            return Ok(false);
        }
        if !event::poll(Duration::from_millis(100)).map_err(|_| PromptError::Io)? {
            continue;
        }
        if let Event::Key(key) = event::read().map_err(|_| PromptError::Io)?
            && let Some(answer) = overwrite_answer(key)
        {
            return Ok(answer);
        }
    }
}

fn overwrite_answer(key: KeyEvent) -> Option<bool> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('c' | 'd'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(false)
        }
        (KeyCode::Enter | KeyCode::Esc, KeyModifiers::NONE)
        | (KeyCode::Char('n' | 'N' | 'н' | 'Н'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(false)
        }
        (KeyCode::Char('y' | 'Y' | 'д' | 'Д'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(true)
        }
        _ => None,
    }
}

struct ConfirmationTerminal;

impl Drop for ConfirmationTerminal {
    /// Restore terminal input even after a cancelled prompt or failed event read.
    fn drop(&mut self) {
        let _ = execute!(io::stderr(), DisableBracketedPaste);
        let _ = terminal::disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwrite_requires_an_explicit_yes_in_either_layout() {
        for (character, expected) in [
            ('y', true),
            ('Y', true),
            ('д', true),
            ('Д', true),
            ('n', false),
            ('N', false),
            ('н', false),
            ('Н', false),
        ] {
            let modifiers = if character.is_uppercase() {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            };
            assert_eq!(
                overwrite_answer(KeyEvent::new(KeyCode::Char(character), modifiers)),
                Some(expected)
            );
        }
        for code in [KeyCode::Enter, KeyCode::Esc] {
            assert_eq!(
                overwrite_answer(KeyEvent::new(code, KeyModifiers::NONE)),
                Some(false)
            );
        }
        for code in [KeyCode::Char('c'), KeyCode::Char('d')] {
            assert_eq!(
                overwrite_answer(KeyEvent::new(code, KeyModifiers::CONTROL)),
                Some(false)
            );
        }
    }

    #[test]
    fn old_numbers_and_non_deliberate_key_events_never_confirm() {
        for code in [KeyCode::Char('1'), KeyCode::Char('2'), KeyCode::Char('a')] {
            assert_eq!(
                overwrite_answer(KeyEvent::new(code, KeyModifiers::NONE)),
                None
            );
        }
        for modifiers in [
            KeyModifiers::ALT,
            KeyModifiers::CONTROL,
            KeyModifiers::SUPER,
        ] {
            assert_eq!(
                overwrite_answer(KeyEvent::new(KeyCode::Char('y'), modifiers)),
                None
            );
        }
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            assert_eq!(
                overwrite_answer(KeyEvent::new_with_kind(
                    KeyCode::Char('д'),
                    KeyModifiers::NONE,
                    kind
                )),
                None
            );
        }
    }
}
