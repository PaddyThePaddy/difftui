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
    center: bool,
}

impl LoadingMsg {
    pub fn new(msg: impl ToString) -> Self {
        Self {
            msg: msg.to_string(),
            center: true,
        }
    }

    pub fn center(mut self, center: bool) -> Self {
        self.center = center;
        self
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
            .alignment(if self.center {
                Alignment::Center
            } else {
                Alignment::Left
            })
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect, widgets::StatefulWidget};

    fn render_msg(msg: &str, tick: u32) -> String {
        let area = Rect::new(0, 0, 30, 1);
        let mut buf = Buffer::empty(area);
        let mut state = LoadingMsgState { tick };
        LoadingMsg::new(msg).render(area, &mut buf, &mut state);
        buf.content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn step_increments_tick() {
        let mut s = LoadingMsgState::default();
        assert_eq!(s.tick, 0);
        s.step();
        assert_eq!(s.tick, 1);
        s.step();
        assert_eq!(s.tick, 2);
    }

    #[test]
    fn icon_at_tick_0_is_dash() {
        assert!(render_msg("x", 0).contains("- x"), "tick 0 → '-'");
    }

    #[test]
    fn icon_at_tick_1_is_backslash() {
        assert!(render_msg("x", 1).contains("\\ x"), "tick 1 → '\\'");
    }

    #[test]
    fn icon_at_tick_2_is_pipe() {
        assert!(render_msg("x", 2).contains("| x"), "tick 2 → '|'");
    }

    #[test]
    fn icon_at_tick_3_is_slash() {
        assert!(render_msg("x", 3).contains("/ x"), "tick 3 → '/'");
    }

    #[test]
    fn icon_wraps_at_tick_4() {
        assert!(render_msg("x", 4).contains("- x"), "tick 4 wraps to '-'");
    }

    #[test]
    fn message_appears_in_rendered_output() {
        assert!(render_msg("Loading files", 0).contains("Loading files"));
    }
}
