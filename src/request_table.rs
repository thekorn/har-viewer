//! Component table adapter. Session remains the authority for filtering and selection.
use crate::app::{bytes, duration, phase_color};
use gpui::{App, Context, Entity, Pixels, Window, div, prelude::*, px, relative, rgb};
use gpui_component::{
    ActiveTheme, Sizable, h_flex,
    table::{Column, ColumnSort, TableDelegate, TableState},
    tag::Tag,
    v_flex,
};
use har_viewer::{har::SortColumn, session::Session};

pub struct Requests {
    session: Entity<Option<Session>>,
    columns: Vec<(SortColumn, Column)>,
    waterfall: bool,
}

impl Requests {
    pub fn new(session: Entity<Option<Session>>) -> Self {
        let mut this = Self {
            session,
            columns: Vec::new(),
            waterfall: false,
        };
        this.configure(false);
        this
    }

    pub fn configure(&mut self, waterfall: bool) {
        self.waterfall = waterfall;
        let columns = if waterfall {
            vec![
                (SortColumn::Url, "url", "Request", 300.),
                (
                    SortColumn::Start,
                    "start",
                    "Timeline · elapsed from capture start",
                    680.,
                ),
                (SortColumn::Duration, "time", "Time", 100.),
            ]
        } else {
            vec![
                (SortColumn::Method, "method", "Method", 80.),
                (SortColumn::Url, "url", "Name", 240.),
                (SortColumn::Status, "status", "Status", 80.),
                (SortColumn::Kind, "kind", "Type", 80.),
                (SortColumn::Size, "size", "Size", 90.),
                (SortColumn::Duration, "time", "Time", 100.),
            ]
        };
        self.columns = columns
            .into_iter()
            .map(|(sort, key, name, width)| {
                (
                    sort,
                    Column::new(key, name)
                        .width(px(width))
                        .sortable()
                        .movable(false)
                        .when(sort == SortColumn::Url, |column| {
                            column.paddings(gpui::Edges {
                                top: px(3.),
                                bottom: px(3.),
                                left: px(8.),
                                right: px(8.),
                            })
                        }),
                )
            })
            .collect();
    }

    pub fn set_widths(&mut self, widths: &[Pixels]) {
        // Table refresh reloads column definitions, including their widths.
        for ((_, column), width) in self.columns.iter_mut().zip(widths) {
            column.width = *width;
        }
    }

    pub fn sync_sort(&mut self, cx: &App) {
        if let Some(session) = self.session.read(cx) {
            // Ascending capture time is also the default order. Keep it neutral
            // so the timeline column can cycle back to descending.
            let capture_order = session.sort == SortColumn::Start && !session.descending;
            for (sort, column) in &mut self.columns {
                column.sort = Some(if capture_order || *sort != session.sort {
                    ColumnSort::Default
                } else if session.descending {
                    ColumnSort::Descending
                } else {
                    ColumnSort::Ascending
                });
            }
        }
    }
}

impl TableDelegate for Requests {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, cx: &App) -> usize {
        self.session
            .read(cx)
            .as_ref()
            .map_or(0, |s| s.visible.len())
    }

    fn column(&self, col_ix: usize, _: &App) -> &Column {
        &self.columns[col_ix].1
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let column = self.columns[col_ix].0;
        self.session.update(cx, |session, cx| {
            if let Some(s) = session {
                // Use the component's descending / ascending / original-order cycle.
                s.sort = if sort == ColumnSort::Default {
                    SortColumn::Start
                } else {
                    column
                };
                s.descending = sort == ColumnSort::Descending;
                // The viewer schedules filtering off-thread, including active fulltext.
                cx.notify();
            }
        });
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(s) = self.session.read(cx).as_ref() else {
            return div().into_any_element();
        };
        let e = &s.capture.entries[s.visible[row_ix]];
        let column = self.columns[col_ix].0;
        let cell = h_flex().h_full().text_sm();
        match column {
            SortColumn::Url => v_flex()
                .justify_center()
                .h_full()
                .line_height(px(16.))
                .overflow_hidden()
                .child(div().text_ellipsis().child(if e.path.is_empty() {
                    "/".into()
                } else {
                    e.path.clone()
                }))
                .child(
                    div()
                        .text_xs()
                        .line_height(px(14.))
                        .text_color(cx.theme().muted_foreground)
                        .text_ellipsis()
                        .child(e.host.clone()),
                )
                .into_any_element(),
            SortColumn::Status => cell
                .child(
                    (match e.status {
                        100..=299 => Tag::success(),
                        300..=399 => Tag::info(),
                        400..=499 => Tag::warning(),
                        _ => Tag::danger(),
                    })
                    .small()
                    .child(if e.status == 0 {
                        "Failed".into()
                    } else {
                        e.status.to_string()
                    }),
                )
                .into_any_element(),
            SortColumn::Start if self.waterfall => {
                let start = s
                    .capture
                    .entries
                    .iter()
                    .filter_map(|e| e.start_ms)
                    .fold(f64::INFINITY, f64::min);
                let total = s.capture.duration_ms().max(1.);
                let mut position = e.start_ms.map(|t| (t - start).max(0.)).unwrap_or(0.) / total;
                let mut track = div()
                    .relative()
                    .w_full()
                    .h(px(16.))
                    .bg(cx.theme().muted)
                    .rounded_sm()
                    .overflow_hidden();
                for phase in e.phases() {
                    track = track.child(
                        div()
                            .absolute()
                            .left(relative(position as f32))
                            .h_full()
                            .w(relative((phase.duration_ms / total) as f32))
                            .min_w(px(1.))
                            .bg(rgb(phase_color(phase.name))),
                    );
                    position += phase.duration_ms / total;
                }
                cell.child(track).into_any_element()
            }
            _ => cell
                .child(match column {
                    SortColumn::Method => e.method.clone(),
                    SortColumn::Kind => e.kind.clone(),
                    SortColumn::Size => e.size.map(bytes).unwrap_or("—".into()),
                    SortColumn::Duration => duration(e.duration_ms),
                    _ => String::new(),
                })
                .into_any_element(),
        }
    }
}
