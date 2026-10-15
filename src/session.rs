//! Request navigation independent of the window system.
use crate::har::{Capture, Filter, SortColumn, Summary};
use std::sync::Arc;

pub struct Session {
    pub capture: Arc<Capture>,
    pub filter: Filter,
    pub sort: SortColumn,
    pub descending: bool,
    pub visible: Vec<usize>,
    pub selected: Option<usize>,
    pub summary: Summary,
}

impl Session {
    pub fn new(capture: Capture) -> Self {
        let mut session = Self {
            capture: Arc::new(capture),
            filter: Filter::default(),
            sort: SortColumn::Start,
            descending: false,
            visible: Vec::new(),
            selected: None,
            summary: Summary::default(),
        };
        session.refresh();
        session
    }

    pub fn refresh(&mut self) {
        self.visible = self
            .capture
            .filtered(&self.filter, self.sort, self.descending);
        self.summary = self.capture.summary(&self.visible);
        if self.selected.is_none_or(|id| !self.visible.contains(&id)) {
            self.selected = self.visible.first().copied();
        }
    }

    pub fn sort_by(&mut self, column: SortColumn) {
        self.descending = self.sort == column && !self.descending;
        self.sort = column;
        self.refresh();
    }

    pub fn navigate(&mut self, delta: isize) -> Option<usize> {
        if self.visible.is_empty() {
            self.selected = None;
            return None;
        }
        let current = self
            .selected
            .and_then(|id| self.visible.iter().position(|&i| i == id))
            .unwrap_or(0);
        let next = current
            .saturating_add_signed(delta)
            .min(self.visible.len() - 1);
        self.selected = Some(self.visible[next]);
        Some(next)
    }
}
