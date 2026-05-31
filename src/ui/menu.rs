use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::Constraint,
    style::{Color, Style, Stylize as _},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, StatefulWidget},
};

use crate::ui::{Action, Popup, tui};

#[derive(Debug, Clone)]
pub struct Menu {
    id: String,
    title: String,
    opts: Vec<(String, Option<char>)>,
    sel: ListState,
    vim_key: bool,
}

impl Menu {
    pub fn new(title: String, opts: Vec<(String, Option<char>)>) -> Self {
        Self {
            id: title.clone(),
            title,
            opts,
            sel: ListState::default(),
            vim_key: false,
        }
    }
    pub fn new_with_id(id: String, title: String, opts: Vec<(String, Option<char>)>) -> Self {
        Self {
            id,
            title,
            opts,
            sel: ListState::default(),
            vim_key: false,
        }
    }

    pub fn select(mut self, i: Option<usize>) -> Self {
        self.sel.select(i);
        self
    }

    /// Enable j/k as up/down
    /// The caller should avoid using these characters as option
    pub fn vim_key(mut self, enable: bool) -> Self {
        self.vim_key = enable;
        self
    }
}

impl Popup for Menu {
    fn handler(&mut self, event: &crate::ui::tui::Event) -> Option<Action> {
        if let tui::Event::Key(event) = event {
            for (option, key) in self
                .opts
                .iter()
                .filter_map(|(opt, key)| key.map(|k| (opt, k)))
            {
                if event.code == KeyCode::Char(key) {
                    return Some(Action::PopupReturn(self.id.clone(), Some(option.clone())));
                }
            }

            match event.code {
                KeyCode::Up => {
                    self.sel.select_previous();
                }
                KeyCode::Down => {
                    self.sel.select_next();
                }
                KeyCode::Enter => {
                    if let Some((opt, _)) = self.sel.selected().and_then(|i| self.opts.get(i)) {
                        return Some(Action::PopupReturn(self.id.clone(), Some(opt.clone())));
                    }
                }
                KeyCode::Esc => {
                    return Some(Action::PopupReturn(self.id.clone(), None));
                }
                KeyCode::Char('j') if self.vim_key => {
                    self.sel.select_next();
                }
                KeyCode::Char('k') if self.vim_key => {
                    self.sel.select_previous();
                }
                _ => {}
            }
        }
        None
    }

    fn render(&mut self, frame: &mut Frame) {
        let hor = Constraint::Length(
            self.opts
                .iter()
                .map(|(o, _)| o.len() + 6) // 2 for keycode, 2 for box border, 2 for padding
                .max()
                .unwrap_or(2) as u16,
        );
        let ver = Constraint::Length((self.opts.len() + 2) as u16);
        let (area, buf) = self.prepare(frame, hor, ver);

        let list_items = self.opts.iter().map(|(o, k)| {
            let key_indicator = if let Some(key) = k {
                Span::from(format!("{key} ")).fg(Color::Red)
            } else {
                Span::from("  ")
            };
            let text = Line::from(vec![
                Span::from(" "),
                key_indicator,
                Span::from(o),
                Span::from(" "),
            ]);
            ListItem::new(text)
        });

        StatefulWidget::render(
            List::new(list_items)
                .block(
                    Block::bordered()
                        .border_type(ratatui::widgets::BorderType::Rounded)
                        .border_style(Style::default().green())
                        .title(self.title.as_str()),
                )
                .highlight_style(Style::default().bg(Color::Blue)),
            area,
            buf,
            &mut self.sel,
        );
    }
}
