use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Stylize,
    text::Line,
    widgets::{Clear, StatefulWidget, Widget},
};

// Tabline that always keep the current tab on the screen
#[derive(Debug, Default)]
pub struct Tabline<'a> {
    tabs: Vec<Line<'a>>,
    current_tab: Option<usize>,
}

impl<'a> Tabline<'a> {
    pub fn new<Iter>(items: Iter) -> Self
    where
        Iter: IntoIterator,
        Iter::Item: Into<Line<'a>>,
    {
        Self {
            tabs: items.into_iter().map(|i| i.into()).collect(),
            ..Default::default()
        }
    }

    pub fn select(mut self, tab: usize) -> Self {
        self.current_tab = Some(tab);
        self
    }
}

#[derive(Debug, Default)]
pub struct TablineState {
    current_scroll: u16,
}

impl<'a> StatefulWidget for Tabline<'a> {
    type State = TablineState;

    fn render(
        self,
        area: ratatui::prelude::Rect,
        buf: &mut ratatui::prelude::Buffer,
        state: &mut Self::State,
    ) {
        let divider = Line::from(" | ").not_reversed();
        let divider_width = divider.width();
        let fullwidth = (self.tabs.iter().map(|t| t.width()).sum::<usize>()
            + self.tabs.len().saturating_sub(1) * divider_width) as u16;
        let full_area = Rect::new(0, 0, fullwidth, 1);
        let mut drawing_area = full_area;
        let mut full_buf = Buffer::empty(full_area);
        let mut tabs = self.tabs.into_iter().enumerate();
        let mut current_tab_area = None;

        if let Some((idx, mut first)) = tabs.next() {
            let width = first.width() as u16;
            if self.current_tab == Some(idx) {
                first = first.reversed();
                let mut a = drawing_area;
                a.width = width;
                current_tab_area = Some(a);
            }
            first.render(drawing_area, &mut full_buf);
            drawing_area.x += width;
        }

        for (idx, mut tab) in tabs {
            divider.clone().render(drawing_area, &mut full_buf);
            drawing_area.x += divider_width as u16;

            let width = tab.width() as u16;
            if self.current_tab == Some(idx) {
                tab = tab.reversed();
                let mut a = drawing_area;
                a.width = width;
                current_tab_area = Some(a);
            }
            tab.render(drawing_area, &mut full_buf);
            drawing_area.x += width;
        }

        if let Some(current_tab_area) = current_tab_area {
            let current_tab_limit = current_tab_area.x + current_tab_area.width;
            let current_tab_start = current_tab_area.x;
            if state.current_scroll > current_tab_start {
                state.current_scroll = current_tab_start;
            }

            if state.current_scroll + area.width < current_tab_limit {
                state.current_scroll = current_tab_limit.saturating_sub(area.width);
            }
        }

        let full_content_x = (full_area.x + state.current_scroll)..(full_area.x + full_area.width);
        let display_buf_x = area.x..(area.x + area.width);

        Clear.render(area, buf);
        for (fx, dx) in full_content_x.zip(display_buf_x) {
            if let Some((dcell, fcell)) = buf
                .cell_mut(Position::new(dx, 0))
                .zip(full_buf.cell(Position::new(fx, 0)))
            {
                *dcell = fcell.clone();
            }
        }
    }
}

impl<'a> Widget for Tabline<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        StatefulWidget::render(self, area, buf, &mut TablineState::default());
    }
}
