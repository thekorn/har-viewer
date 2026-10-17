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

#[derive(Clone, Copy, Debug, Default)]
pub struct HistogramBin {
    pub requests: u32,
    pub errors: u32,
    first_row: Option<usize>,
}

/// Cheap, immutable inputs for filtering off the UI thread; the capture is shared.
pub struct RefreshRequest {
    capture: Arc<Capture>,
    filter: Filter,
    sort: SortColumn,
    descending: bool,
}

pub struct RefreshResult {
    request: RefreshRequest,
    visible: Vec<usize>,
    summary: Summary,
}

impl RefreshRequest {
    pub fn is_current(&self, session: &Session) -> bool {
        Arc::ptr_eq(&self.capture, &session.capture)
            && self.filter == session.filter
            && self.sort == session.sort
            && self.descending == session.descending
    }

    /// Run on a worker: scanning, decoding, sorting and aggregation can be expensive.
    pub fn run(self) -> RefreshResult {
        let visible = self
            .capture
            .filtered(&self.filter, self.sort, self.descending);
        let summary = self.capture.summary(&visible);
        RefreshResult {
            request: self,
            visible,
            summary,
        }
    }
}

pub struct Session {
    pub capture: Arc<Capture>,
    pub filter: Filter,
    pub sort: SortColumn,
    pub descending: bool,
    pub visible: Vec<usize>,
    pub selected: Option<usize>,
    pub summary: Summary,
    applied: Option<RefreshRequest>,
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
            applied: None,
        };
        session.refresh();
        session
    }

    pub fn refresh(&mut self) {
        self.apply_refresh(self.refresh_request().run());
    }

    pub fn needs_refresh(&self) -> bool {
        self.applied
            .as_ref()
            .is_none_or(|request| !request.is_current(self))
    }

    pub fn refresh_request(&self) -> RefreshRequest {
        RefreshRequest {
            capture: self.capture.clone(),
            filter: self.filter.clone(),
            sort: self.sort,
            descending: self.descending,
        }
    }

    /// Reject stale inputs, and preserve selection made while the worker was busy.
    pub fn apply_refresh(&mut self, result: RefreshResult) -> bool {
        if !result.request.is_current(self) {
            return false;
        }
        self.applied = Some(result.request);
        self.visible = result.visible;
        self.summary = result.summary;
        if self.selected.is_none_or(|id| !self.visible.contains(&id)) {
            self.selected = self.visible.first().copied();
        }
        true
    }

    pub fn sort_by(&mut self, column: SortColumn) {
        self.descending = self.sort == column && !self.descending;
        self.sort = column;
        self.refresh();
    }

    /// Fixed capture-wide buckets, with missing timestamps displayed at zero.
    pub fn histogram(&self) -> [HistogramBin; 64] {
        let mut bins = [HistogramBin::default(); 64];
        let start = self
            .capture
            .entries
            .iter()
            .filter_map(|e| e.start_ms)
            .fold(f64::INFINITY, f64::min);
        let span = self.capture.duration_ms().max(1.);
        for (row, &id) in self.visible.iter().enumerate() {
            let entry = &self.capture.entries[id];
            let offset = entry.start_ms.map(|t| (t - start).max(0.)).unwrap_or(0.);
            let index = ((offset / span * (bins.len() - 1) as f64) as usize).min(bins.len() - 1);
            let bin = &mut bins[index];
            bin.requests += 1;
            if entry.status == 0 || entry.status >= 400 {
                bin.errors += 1;
            }
            bin.first_row.get_or_insert(row);
        }
        bins
    }

    /// Select the first matching row in the current table order, without filtering.
    /// Empty or invalid buckets leave selection unchanged.
    pub fn select_histogram_bin(&mut self, index: usize) -> Option<usize> {
        let row = self.histogram().get(index)?.first_row?;
        self.selected = Some(self.visible[row]);
        Some(row)
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
