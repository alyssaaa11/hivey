// hivey: interactive picker for `hivey session list` in a terminal. Click a
// session (or move with the arrow keys and press Enter) to open it.

use std::io::{self, Write};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{
        self, disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};

use crate::session::SessionInfo;

const TITLE: &str = "hivey sessions: click one or press Enter to open it · ↑/↓ move · q quit";
/// Screen row of the column header; session rows start right below it.
const HEADER_ROW: u16 = 2;
const FIRST_ROW: u16 = HEADER_ROW + 1;

/// Shows the picker and returns the chosen session name, or `None` when the
/// user quits without choosing.
pub(super) fn pick(sessions: &[SessionInfo]) -> io::Result<Option<String>> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    let setup = queue!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide)
        .and_then(|()| stdout.flush());
    let result = setup.and_then(|()| run(&mut stdout, sessions));
    let _ = queue!(stdout, Show, DisableMouseCapture, LeaveAlternateScreen);
    let _ = stdout.flush();
    let _ = disable_raw_mode();
    result
}

fn run(stdout: &mut io::Stdout, sessions: &[SessionInfo]) -> io::Result<Option<String>> {
    let mut picker = Picker::new(sessions.len());
    loop {
        let (_, height) = terminal::size()?;
        picker.fit(visible_rows(height));
        draw(stdout, sessions, &picker)?;
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Up | KeyCode::Char('k') => picker.move_by(-1),
                KeyCode::Down | KeyCode::Char('j') => picker.move_by(1),
                KeyCode::Home => picker.selected = 0,
                KeyCode::End => picker.selected = sessions.len().saturating_sub(1),
                KeyCode::Enter => return Ok(Some(sessions[picker.selected].name.clone())),
                KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None)
                }
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(index) = picker.row_at(mouse.row, sessions.len()) {
                        return Ok(Some(sessions[index].name.clone()));
                    }
                }
                MouseEventKind::Moved => {
                    if let Some(index) = picker.row_at(mouse.row, sessions.len()) {
                        picker.selected = index;
                    }
                }
                MouseEventKind::ScrollUp => picker.move_by(-1),
                MouseEventKind::ScrollDown => picker.move_by(1),
                _ => {}
            },
            _ => {}
        }
    }
}

fn visible_rows(height: u16) -> usize {
    usize::from(height.saturating_sub(FIRST_ROW)).max(1)
}

/// Selection and scroll state, kept apart from drawing so it can be tested.
#[derive(Debug)]
struct Picker {
    len: usize,
    selected: usize,
    offset: usize,
    visible: usize,
}

impl Picker {
    fn new(len: usize) -> Self {
        Self {
            len,
            selected: 0,
            offset: 0,
            visible: 1,
        }
    }

    fn move_by(&mut self, delta: isize) {
        let last = self.len.saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
        self.fit(self.visible);
    }

    /// Scrolls so the selected row stays on screen.
    fn fit(&mut self, visible: usize) {
        self.visible = visible.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + self.visible {
            self.offset = self.selected + 1 - self.visible;
        }
    }

    /// The session index drawn on screen row `row`, if any.
    fn row_at(&self, row: u16, len: usize) -> Option<usize> {
        let line = usize::from(row.checked_sub(FIRST_ROW)?);
        if line >= self.visible {
            return None;
        }
        let index = self.offset + line;
        (index < len).then_some(index)
    }
}

fn status(session: &SessionInfo) -> (&'static str, Color) {
    if session.connection_error.is_some() {
        ("unavailable", Color::Red)
    } else if session.running {
        ("running", Color::Green)
    } else {
        ("stopped", Color::DarkGrey)
    }
}

fn fit_width(text: String, width: usize) -> String {
    text.chars().take(width).collect()
}

fn draw(stdout: &mut io::Stdout, sessions: &[SessionInfo], picker: &Picker) -> io::Result<()> {
    let width = usize::from(terminal::size()?.0);
    let name_width = sessions
        .iter()
        .map(|session| session.name.chars().count())
        .max()
        .unwrap_or(0)
        .max(4);
    let row_text = |name: &str, status: &str, directory: &str| {
        fit_width(
            format!("  {name:<name_width$}  {status:<11}  {directory}"),
            width,
        )
    };

    queue!(
        stdout,
        Clear(ClearType::All),
        MoveTo(0, 0),
        SetAttribute(Attribute::Bold),
        Print(fit_width(TITLE.to_string(), width)),
        SetAttribute(Attribute::Reset),
        MoveTo(0, HEADER_ROW),
        SetAttribute(Attribute::Dim),
        Print(row_text("name", "status", "directory")),
        SetAttribute(Attribute::Reset),
    )?;

    let shown = sessions
        .iter()
        .enumerate()
        .skip(picker.offset)
        .take(picker.visible);
    for (line, (index, session)) in shown.enumerate() {
        let (label, color) = status(session);
        let row = FIRST_ROW + u16::try_from(line).unwrap_or(u16::MAX);
        let text = row_text(&session.name, label, &session.session_dir);
        queue!(stdout, MoveTo(0, row))?;
        if index == picker.selected {
            queue!(
                stdout,
                SetAttribute(Attribute::Reverse),
                Print(format!("{text:<width$}")),
                SetAttribute(Attribute::Reset),
            )?;
        } else {
            queue!(stdout, SetForegroundColor(color), Print(text), ResetColor)?;
        }
    }
    stdout.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_stays_in_bounds() {
        let mut picker = Picker::new(3);
        picker.move_by(-1);
        assert_eq!(picker.selected, 0);
        picker.move_by(5);
        assert_eq!(picker.selected, 2);
    }

    #[test]
    fn selection_scrolls_into_view() {
        let mut picker = Picker::new(10);
        picker.fit(3);
        picker.move_by(4);
        assert_eq!((picker.selected, picker.offset), (4, 2));
        picker.move_by(-4);
        assert_eq!((picker.selected, picker.offset), (0, 0));
    }

    #[test]
    fn clicks_map_to_visible_sessions() {
        let mut picker = Picker::new(10);
        picker.fit(3);
        picker.move_by(5);
        assert_eq!(picker.row_at(HEADER_ROW, 10), None);
        assert_eq!(picker.row_at(FIRST_ROW, 10), Some(3));
        assert_eq!(picker.row_at(FIRST_ROW + 2, 10), Some(5));
        assert_eq!(picker.row_at(FIRST_ROW + 3, 10), None);
    }

    #[test]
    fn clicks_below_the_last_session_are_ignored() {
        let mut picker = Picker::new(2);
        picker.fit(10);
        assert_eq!(picker.row_at(FIRST_ROW + 1, 2), Some(1));
        assert_eq!(picker.row_at(FIRST_ROW + 2, 2), None);
    }
}
