use ratatui::{
    layout::Alignment,
    widgets::{Paragraph, StatefulWidget, Widget},
};

#[derive(Debug, Default, Clone, Copy)]
pub struct LoadingMsgState {
    tick: u32,
}

impl LoadingMsgState {
    pub fn step(&mut self) {
        self.tick += 1;
    }
}

#[derive(Debug, Default, Clone)]
pub struct LoadingMsg {
    msg: String,
}

impl LoadingMsg {
    pub fn new(msg: impl ToString) -> Self {
        Self {
            msg: msg.to_string(),
        }
    }
}

impl StatefulWidget for LoadingMsg {
    type State = LoadingMsgState;

    fn render(
        self,
        area: ratatui::prelude::Rect,
        buf: &mut ratatui::prelude::Buffer,
        state: &mut Self::State,
    ) {
        let icon = match state.tick % 4 {
            0 => "-",
            1 => "\\",
            2 => "|",
            3 => "/",
            _ => unreachable!(),
        };
        let s = format!("{} {}", icon, self.msg);
        Paragraph::new(s)
            .alignment(Alignment::Center)
            .render(area, buf);
    }
}
