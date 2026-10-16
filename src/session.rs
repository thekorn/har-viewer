//! Request navigation independent of the window system.
use crate::har::{Capture, Filter, SortColumn, Summary};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainSort {
    Host,
    Requests,
    Size,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainSummary {
    pub host: String,
    pub requests: usize,
    pub bytes: u64,
}

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

    /// Aggregate only visible requests. Equal metrics sort by host A–Z in either direction.
    pub fn domains(&self, sort: DomainSort, descending: bool) -> Vec<DomainSummary> {
        let mut domains = BTreeMap::<String, DomainSummary>::new();
        for &id in &self.visible {
            let entry = &self.capture.entries[id];
            let domain = domains
                .entry(entry.host.clone())
                .or_insert_with(|| DomainSummary {
                    host: entry.host.clone(),
                    requests: 0,
                    bytes: 0,
                });
            domain.requests += 1;
            domain.bytes = domain.bytes.saturating_add(entry.size.unwrap_or(0));
        }
        let mut domains: Vec<_> = domains.into_values().collect();
        domains.sort_by(|a, b| {
            let order = match sort {
                DomainSort::Host => a.host.cmp(&b.host),
                DomainSort::Requests => a.requests.cmp(&b.requests),
                DomainSort::Size => a.bytes.cmp(&b.bytes),
            };
            let order = if descending { order.reverse() } else { order };
            order.then_with(|| a.host.cmp(&b.host))
        });
        domains
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
