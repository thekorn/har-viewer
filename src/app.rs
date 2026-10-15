use crate::input::{Changed, TextInput};
use crate::platform::{MONO_FONT, PRIMARY_LABEL, PRIMARY_MODIFIER, UI_FONT};
use gpui::{
    App, ClipboardItem, Context, Entity, ExternalPaths, FocusHandle, Focusable, KeyBinding,
    PathPromptOptions, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, Window,
    actions, div, prelude::*, px, relative, rgb, uniform_list,
};
use har_viewer::{
    har::{Capture, Entry, SortColumn},
    session::Session,
};
use std::{collections::BTreeMap, io::Read, path::PathBuf};

const BG: u32 = 0x101014;
const PANEL: u32 = 0x15151b;
const BORDER: u32 = 0x292932;
const MUTED: u32 = 0x90909e;
const TEXT: u32 = 0xe5e5ee;
const ACCENT: u32 = 0xa399ff;
const SELECTED: u32 = 0x29253e;

actions!(
    viewer,
    [
        Open, Search, Next, Previous, Escape, Submit, CopyUrl, CopyHar, Quit
    ]
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Requests,
    Waterfall,
    Overview,
    Issues,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Headers,
    Payload,
    Response,
    Cookies,
    Timing,
    Raw,
}
#[derive(Clone, Copy)]
enum Facet {
    Status,
    Method,
    Kind,
    Host,
}

pub struct Viewer {
    session: Option<Session>,
    filename: String,
    search: Entity<TextInput>,
    path: Entity<TextInput>,
    focus: FocusHandle,
    _subscription: Subscription,
    view: View,
    tab: Tab,
    inspector: bool,
    open_path: bool,
    loading: bool,
    load_generation: u64,
    error: Option<String>,
    notice: String,
    lines: Vec<SharedString>,
    list_scroll: UniformListScrollHandle,
    detail_scroll: UniformListScrollHandle,
}

pub fn init(cx: &mut App) {
    crate::input::init(cx);
    cx.bind_keys([
        KeyBinding::new(&format!("{PRIMARY_MODIFIER}-o"), Open, Some("Viewer")),
        KeyBinding::new(&format!("{PRIMARY_MODIFIER}-f"), Search, Some("Viewer")),
        KeyBinding::new("down", Next, Some("Viewer && !TextInput")),
        KeyBinding::new("up", Previous, Some("Viewer && !TextInput")),
        KeyBinding::new("escape", Escape, Some("Viewer")),
        KeyBinding::new("enter", Submit, Some("Viewer")),
        KeyBinding::new(
            &format!("{PRIMARY_MODIFIER}-shift-c"),
            CopyUrl,
            Some("Viewer"),
        ),
        KeyBinding::new(
            &format!("{PRIMARY_MODIFIER}-shift-e"),
            CopyHar,
            Some("Viewer"),
        ),
        KeyBinding::new(&format!("{PRIMARY_MODIFIER}-q"), Quit, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

impl Viewer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(
                &format!("Filter URL, method or status…  {PRIMARY_LABEL} F"),
                cx,
            )
        });
        let path = cx.new(|cx| TextInput::new("/path/to/capture.har", cx));
        let subscription = cx.subscribe(&search, |this, input, _: &Changed, cx| {
            if let Some(s) = &mut this.session {
                s.filter.query = input.read(cx).text().to_owned();
                s.refresh();
                this.refresh_detail();
                this.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
        });
        Self {
            session: None,
            filename: String::new(),
            search,
            path,
            focus: cx.focus_handle(),
            _subscription: subscription,
            view: View::Requests,
            tab: Tab::Headers,
            inspector: true,
            open_path: false,
            loading: false,
            load_generation: 0,
            error: None,
            notice: "Local only. No uploads, telemetry or replay.".into(),
            lines: Vec::new(),
            list_scroll: UniformListScrollHandle::new(),
            detail_scroll: UniformListScrollHandle::new(),
        }
    }

    pub fn sample(&mut self, cx: &mut Context<Self>) {
        self.load_generation += 1;
        self.install(Capture::sample(), "sample-shop.har".into(), cx);
    }

    fn install(&mut self, capture: Capture, filename: String, cx: &mut Context<Self>) {
        self.notice = if capture.warnings.is_empty() {
            "Capture loaded · Everything stays on this device".into()
        } else {
            format!(
                "{} validation warnings · See Issues",
                capture.warnings.len()
            )
        };
        self.session = Some(Session::new(capture));
        self.filename = filename;
        self.loading = false;
        self.open_path = false;
        self.error = None;
        self.view = View::Requests;
        self.inspector = true;
        self.search.update(cx, |input, cx| input.set_text("", cx));
        self.refresh_detail();
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    pub fn load(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.load_generation += 1;
        let generation = self.load_generation;
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let task = cx.background_executor().spawn(async move {
            // Bound reads too, not just metadata: files can grow between stat and read.
            const LIMIT: u64 = 1024 * 1024 * 1024;
            let file = std::fs::File::open(&path)
                .map_err(|e| format!("Could not open {}: {e}", path.display()))?;
            let mut bytes = Vec::new();
            file.take(LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| format!("Could not read file: {e}"))?;
            if bytes.len() as u64 > LIMIT {
                return Err("Capture exceeds the 1 GiB limit for this iteration.".into());
            }
            let text = String::from_utf8(bytes).map_err(|_| "HAR must be UTF-8 JSON".to_owned())?;
            Capture::parse(text.trim_start_matches('\u{feff}'))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.load_generation {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(capture) => this.install(capture, filename, cx),
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn open(&mut self, _: &Open, window: &mut Window, cx: &mut Context<Self>) {
        self.open_path = true;
        window.focus(&self.path.focus_handle(cx));
        cx.notify();
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open HAR capture".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = receiver.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(paths))) => if let Some(path) = paths.into_iter().next() { this.load(path, cx); },
                    Ok(Ok(None)) => {},
                    _ => { this.error = Some("File picker unavailable. Enter a full path above instead (Linux requires an XDG desktop portal).".into()); }
                }
                cx.notify();
            });
        }).detach();
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_path {
            let path = self.path.read(cx).text().to_owned();
            if !path.is_empty() {
                self.load(PathBuf::from(path), cx);
            }
        } else {
            window.focus(&self.focus);
        }
    }

    fn search_action(&mut self, _: &Search, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_some() {
            self.view = View::Requests;
            window.focus(&self.search.focus_handle(cx));
            cx.notify();
        }
    }

    fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        self.open_path = false;
        self.error = None;
        window.focus(&self.focus);
        cx.notify();
    }

    fn next(&mut self, _: &Next, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(1, cx);
    }
    fn previous(&mut self, _: &Previous, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(-1, cx);
    }
    fn navigate(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.session {
            if let Some(index) = s.navigate(delta) {
                self.list_scroll
                    .scroll_to_item(index, ScrollStrategy::Center);
            }
            self.inspector = true;
            self.refresh_detail();
            cx.notify();
        }
    }

    fn selected(&self) -> Option<&Entry> {
        let s = self.session.as_ref()?;
        s.capture.entries.get(s.selected?)
    }

    fn copy_url(&mut self, _: &CopyUrl, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(e) = self.selected() {
            cx.write_to_clipboard(ClipboardItem::new_string(e.url.clone()));
            self.notice = "URL copied".into();
            cx.notify();
        }
    }

    fn copy_har(&mut self, _: &CopyHar, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = &self.session {
            cx.write_to_clipboard(ClipboardItem::new_string(s.capture.export(&s.visible)));
            self.notice = "HAR copied · Credentials are NOT redacted; review before sharing".into();
            cx.notify();
        }
    }

    fn refresh_detail(&mut self) {
        self.lines = self.selected().map(|e| {
            let text = detail_content(e, self.tab).unwrap_or_else(|error| error);
            let text = if text.is_empty() { "No body recorded in this HAR.".into() } else { text };
            // Virtualize rows and cap preview size. Copy retains the complete value.
            let mut lines: Vec<SharedString> = text.lines().take(5000)
                .map(|line| line.chars().take(2000).collect::<String>().into()).collect();
            if text.lines().count() > 5000 || text.lines().any(|l| l.chars().count() > 2000) {
                lines.insert(0, "[Preview limited to 5,000 lines / 2,000 characters per line. Copy for full content.]".into());
            }
            lines
        }).unwrap_or_default();
        self.detail_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.session {
            s.filter = Default::default();
            s.refresh();
        }
        self.search.update(cx, |input, cx| input.set_text("", cx));
        self.refresh_detail();
        cx.notify();
    }

    fn toggle_facet(&mut self, facet: Facet, value: &str, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.session {
            match facet {
                Facet::Status => {
                    let status = value.parse::<u16>().unwrap_or(0);
                    if !s.filter.statuses.remove(&status) {
                        s.filter.statuses.insert(status);
                    }
                }
                _ => {
                    let set = match facet {
                        Facet::Method => &mut s.filter.methods,
                        Facet::Kind => &mut s.filter.kinds,
                        _ => &mut s.filter.hosts,
                    };
                    if !set.remove(value) {
                        set.insert(value.into());
                    }
                }
            }
            s.refresh();
            self.refresh_detail();
            self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
            cx.notify();
        }
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let Some(s) = &self.session else {
            return;
        };
        let capture = s.capture.clone();
        let indices = s.visible.clone();
        let receiver = cx.prompt_for_new_path(
            &std::env::current_dir().unwrap_or_default(),
            Some("filtered.har"),
        );
        cx.spawn(async move |this, cx| {
            let result = match receiver.await {
                Ok(Ok(Some(path))) => {
                    let task = cx.background_executor().spawn(async move {
                        // Never silently overwrite an existing capture, even if a portal allows it.
                        use std::io::Write;
                        let data = capture.export(&indices);
                        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)
                            .map_err(|e| format!("Export failed (choose a new filename): {e}"))?;
                        file.write_all(data.as_bytes()).map_err(|e| format!("Export failed: {e}"))
                    });
                    task.await.map(|()| "Exported visible requests. HAR includes credentials; review before sharing.".to_owned())
                },
                Ok(Ok(None)) => return,
                _ => Err("Save dialog unavailable. Use Copy HAR in the Overview, then save from your editor.".into()),
            };
            let _ = this.update(cx, |this, cx| { match result { Ok(message) => this.notice = message, Err(error) => this.error = Some(error) }; cx.notify(); });
        }).detach();
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        row()
            .h(px(58.))
            .px_5()
            .gap_3()
            .bg(rgb(PANEL))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .size(px(30.))
                    .rounded_md()
                    .bg(rgb(ACCENT))
                    .text_color(rgb(BG))
                    .font_weight(gpui::FontWeight::BOLD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child("H"),
            )
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_size(px(15.))
                    .child("HAR Viewer"),
            )
            .child(div().text_color(rgb(MUTED)).child("/"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(if self.filename.is_empty() {
                        "Network inspection, without the noise".into()
                    } else {
                        self.filename.clone()
                    }),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(0x7bd9ac))
                    .child("●  LOCAL ONLY"),
            )
            .child(
                button("open", &format!("Open HAR  {PRIMARY_LABEL} O"))
                    .on_click(cx.listener(|this, _, window, cx| this.open(&Open, window, cx))),
            )
            .when(self.session.is_some(), |d| {
                d.child(
                    button("export", "Export HAR")
                        .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                )
            })
    }

    fn navigation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        row()
            .h(px(42.))
            .px_5()
            .gap_1()
            .border_b_1()
            .border_color(rgb(BORDER))
            .children(
                [
                    (View::Requests, "Requests"),
                    (View::Waterfall, "Waterfall"),
                    (View::Overview, "Overview"),
                    (View::Issues, "Issues"),
                ]
                .into_iter()
                .enumerate()
                .map(|(i, (view, label))| {
                    div()
                        .id(("view", i))
                        .h_full()
                        .px_4()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .border_b_2()
                        .border_color(rgb(if self.view == view { ACCENT } else { BG }))
                        .text_color(rgb(if self.view == view { TEXT } else { MUTED }))
                        .hover(|d| d.bg(rgb(PANEL)).text_color(rgb(TEXT)))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.view = view;
                            cx.notify();
                        }))
                }),
            )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.session.as_ref().unwrap();
        let mut groups = Vec::new();
        for (group_id, facet, title) in [
            (0, Facet::Status, "STATUS"),
            (1, Facet::Method, "METHOD"),
            (2, Facet::Kind, "RESOURCE TYPE"),
            (3, Facet::Host, "DOMAIN"),
        ] {
            let mut counts = BTreeMap::<String, usize>::new();
            for e in &s.capture.entries {
                let key = match facet {
                    Facet::Status => (e.status / 100).to_string(),
                    Facet::Method => e.method.clone(),
                    Facet::Kind => e.kind.clone(),
                    Facet::Host => e.host.clone(),
                };
                *counts.entry(key).or_default() += 1;
            }
            let group = col()
                .px_3()
                .py_3()
                .gap_1()
                .border_b_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        .px_2()
                        .pb_2()
                        .text_size(px(10.))
                        .text_color(rgb(MUTED))
                        .child(title),
                )
                .children(counts.into_iter().enumerate().map(|(index, (key, count))| {
                    let active = match facet {
                        Facet::Status => s.filter.statuses.contains(&key.parse().unwrap_or(0)),
                        Facet::Method => s.filter.methods.contains(&key),
                        Facet::Kind => s.filter.kinds.contains(&key),
                        Facet::Host => s.filter.hosts.contains(&key),
                    };
                    let label = if matches!(facet, Facet::Status) {
                        match key.as_str() {
                            "0" => "Failed",
                            "1" => "1xx · Informational",
                            "2" => "2xx · Success",
                            "3" => "3xx · Redirect",
                            "4" => "4xx · Client error",
                            _ => "5xx · Server error",
                        }
                        .to_owned()
                    } else if key.is_empty() {
                        "Unknown".into()
                    } else {
                        key.clone()
                    };
                    row()
                        .id((SharedString::from(format!("facet-{group_id}")), index))
                        .h(px(29.))
                        .px_2()
                        .gap_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .bg(rgb(if active { SELECTED } else { PANEL }))
                        .hover(|d| d.bg(rgb(0x23232c)))
                        .child(
                            div()
                                .text_color(rgb(if active { ACCENT } else { MUTED }))
                                .child(if active { "☑" } else { "☐" }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_size(px(11.))
                                .child(label),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(rgb(MUTED))
                                .child(count.to_string()),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.toggle_facet(facet, &key, cx)),
                        )
                }));
            groups.push(group);
        }
        col()
            .w(px(210.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                row()
                    .h(px(46.))
                    .px_4()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child("Filters")
                    .child(
                        button("reset", "Reset")
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                    ),
            )
            .child(
                col()
                    .id("facets")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(groups)
                    .child(
                        button(
                            "slow",
                            if s.filter.slow_only {
                                "✓  Only slow ≥ 1 s"
                            } else {
                                "Only slow ≥ 1 s"
                            },
                        )
                        .m_3()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(s) = &mut this.session {
                                s.filter.slow_only = !s.filter.slow_only;
                                s.refresh();
                            }
                            this.refresh_detail();
                            cx.notify();
                        })),
                    ),
            )
    }

    fn histogram(&self) -> impl IntoElement {
        let s = self.session.as_ref().unwrap();
        let mut bins = [(0usize, 0usize); 64];
        let start = s
            .capture
            .entries
            .iter()
            .filter_map(|e| e.start_ms)
            .fold(f64::INFINITY, f64::min);
        let span = s.capture.duration_ms().max(1.);
        for &i in &s.visible {
            let e = &s.capture.entries[i];
            let bin =
                (((e.start_ms.unwrap_or(start) - start) / span * 63.).max(0.) as usize).min(63);
            bins[bin].0 += 1;
            if e.status == 0 || e.status >= 400 {
                bins[bin].1 += 1;
            }
        }
        let max = bins.iter().map(|b| b.0).max().unwrap_or(1).max(1) as f32;
        col()
            .p_3()
            .gap_2()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                row()
                    .justify_between()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child("REQUESTS OVER TIME")
                    .child(format!("0 — {}", duration(span))),
            )
            .child(
                row()
                    .h(px(34.))
                    .items_end()
                    .gap(px(3.))
                    .children(bins.into_iter().map(|(count, errors)| {
                        div()
                            .flex_1()
                            .h(px((count as f32 / max * 30.).max(2.)))
                            .rounded_t_sm()
                            .bg(rgb(if errors > 0 {
                                0xe89973
                            } else if count > 0 {
                                0x7167b2
                            } else {
                                0x23232d
                            }))
                    })),
            )
    }

    fn request_table(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.session.as_ref().unwrap();
        let waterfall = self.view == View::Waterfall;
        let show_extra = !self.inspector || waterfall;
        let headers: Vec<(SortColumn, &str, f32)> = if waterfall {
            vec![
                (SortColumn::Url, "Request", 260.),
                (
                    SortColumn::Start,
                    "Timeline · elapsed from capture start",
                    0.,
                ),
            ]
        } else {
            let mut columns = vec![
                (SortColumn::Method, "Method", 65.),
                (SortColumn::Url, "Name", 0.),
                (SortColumn::Status, "Status", 60.),
            ];
            if show_extra {
                columns.extend([
                    (SortColumn::Kind, "Type", 55.),
                    (SortColumn::Size, "Size", 80.),
                ]);
            }
            columns.push((SortColumn::Duration, "Time", 80.));
            columns
        };
        col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                row()
                    .h(px(46.))
                    .px_3()
                    .gap_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(div().flex_1().min_w_0().child(self.search.clone()))
                    .child(
                        div()
                            .text_color(rgb(MUTED))
                            .text_size(px(11.))
                            .child(format!("{} requests", s.visible.len())),
                    ),
            )
            .child(self.histogram())
            .when(waterfall, |d| {
                d.child(
                    row().px_4().h(px(30.)).gap_3().text_size(px(10.)).children(
                        [
                            ("DNS", "dns"),
                            ("Connect", "connect"),
                            ("TLS", "ssl"),
                            ("Send", "send"),
                            ("TTFB", "wait"),
                            ("Download", "receive"),
                        ]
                        .into_iter()
                        .map(|(label, key)| div().text_color(rgb(phase_color(key))).child(label)),
                    ),
                )
            })
            .child(
                row()
                    .h(px(32.))
                    .px_3()
                    .bg(rgb(PANEL))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .children(
                        headers
                            .into_iter()
                            .enumerate()
                            .map(|(i, (sort, label, width))| {
                                div()
                                    .id(("sort", i))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .cursor_pointer()
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED))
                                    .when(width > 0., |d| d.w(px(width)).flex_shrink_0())
                                    .when(width == 0., |d| d.flex_1().min_w_0())
                                    .hover(|d| d.text_color(rgb(TEXT)))
                                    .child(format!(
                                        "{label}{}",
                                        if s.sort == sort {
                                            if s.descending { " ↓" } else { " ↑" }
                                        } else {
                                            ""
                                        }
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(s) = &mut this.session {
                                            s.sort_by(sort);
                                        }
                                        this.refresh_detail();
                                        cx.notify();
                                    }))
                            }),
                    ),
            )
            .when(s.visible.is_empty(), |d| {
                d.child(
                    col()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child("No requests match these filters")
                        .child(
                            button("clear-empty", "Clear filters")
                                .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                        ),
                )
            })
            .when(!s.visible.is_empty(), |d| {
                d.child(
                    uniform_list(
                        "requests",
                        s.visible.len(),
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|i| this.request_row(i, waterfall, show_extra, cx))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(self.list_scroll.clone())
                    .flex_1()
                    .min_h_0(),
                )
            })
    }

    fn request_row(
        &self,
        index: usize,
        waterfall: bool,
        show_extra: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let s = self.session.as_ref().unwrap();
        let id = s.visible[index];
        let e = &s.capture.entries[id];
        let selected = s.selected == Some(id);
        row()
            .id(("request", id))
            .w_full()
            .h(px(42.))
            .px_3()
            .border_b_1()
            .border_color(rgb(0x1e1e26))
            .bg(rgb(if selected {
                SELECTED
            } else if index.is_multiple_of(2) {
                BG
            } else {
                0x131318
            }))
            .cursor_pointer()
            .hover(|d| d.bg(rgb(0x24232f)))
            .when(!waterfall, |d| {
                d.child(
                    div()
                        .w(px(65.))
                        .flex_shrink_0()
                        .text_size(px(10.))
                        .text_color(rgb(if e.method == "GET" {
                            0x8abff3
                        } else {
                            0x79dbaa
                        }))
                        .child(e.method.clone()),
                )
            })
            .child(
                col()
                    .min_w_0()
                    .overflow_hidden()
                    .pr_3()
                    .when(waterfall, |d| d.w(px(260.)).flex_shrink_0())
                    .when(!waterfall, |d| d.flex_1())
                    .child(
                        div()
                            .text_ellipsis()
                            .overflow_hidden()
                            .text_size(px(12.))
                            .child(if e.path.is_empty() {
                                "/".into()
                            } else {
                                e.path.clone()
                            }),
                    )
                    .child(
                        div()
                            .text_ellipsis()
                            .overflow_hidden()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child(e.host.clone()),
                    ),
            )
            .when(!waterfall, |d| {
                d.child(
                    div()
                        .w(px(60.))
                        .flex_shrink_0()
                        .text_color(rgb(status_color(e.status)))
                        .child(if e.status == 0 {
                            "Failed".into()
                        } else {
                            e.status.to_string()
                        }),
                )
                .when(show_extra, |d| {
                    d.child(
                        div()
                            .w(px(55.))
                            .flex_shrink_0()
                            .text_color(rgb(MUTED))
                            .child(e.kind.clone()),
                    )
                    .child(
                        div()
                            .w(px(80.))
                            .flex_shrink_0()
                            .child(e.size.map(bytes).unwrap_or("—".into())),
                    )
                })
                .child(
                    div()
                        .w(px(80.))
                        .flex_shrink_0()
                        .text_color(rgb(if e.duration_ms >= 1000. {
                            0xe8b779
                        } else {
                            TEXT
                        }))
                        .child(duration(e.duration_ms)),
                )
            })
            .when(waterfall, |d| {
                let start = s
                    .capture
                    .entries
                    .iter()
                    .filter_map(|e| e.start_ms)
                    .fold(f64::INFINITY, f64::min);
                let total = s.capture.duration_ms().max(1.);
                let offset = e.start_ms.map(|t| (t - start).max(0.)).unwrap_or(0.) / total;
                let mut position = offset;
                let mut track = div()
                    .relative()
                    .flex_1()
                    .h(px(16.))
                    .bg(rgb(0x1c1c25))
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
                d.child(track).child(
                    div()
                        .w(px(78.))
                        .pl_2()
                        .text_size(px(10.))
                        .child(duration(e.duration_ms)),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(s) = &mut this.session {
                    s.selected = Some(id);
                }
                this.inspector = true;
                this.refresh_detail();
                window.focus(&this.focus);
                cx.notify();
            }))
    }

    fn inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let e = self.selected().unwrap();
        col()
            .w(px(440.))
            .flex_shrink_0()
            .min_h_0()
            .border_l_1()
            .border_color(rgb(BORDER))
            .bg(rgb(PANEL))
            .child(
                col()
                    .p_4()
                    .gap_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        row()
                            .gap_3()
                            .child(div().text_color(rgb(0x7ddbb1)).child(e.method.clone()))
                            .child(div().text_color(rgb(status_color(e.status))).child(format!(
                                "{} {}",
                                e.status,
                                e.raw["response"]["statusText"].as_str().unwrap_or("")
                            )))
                            .child(div().flex_1())
                            .child(
                                button("previous", "↑")
                                    .on_click(cx.listener(|this, _, _, cx| this.navigate(-1, cx))),
                            )
                            .child(
                                button("next", "↓")
                                    .on_click(cx.listener(|this, _, _, cx| this.navigate(1, cx))),
                            )
                            .child(button("close", "×").on_click(cx.listener(|this, _, _, cx| {
                                this.inspector = false;
                                cx.notify();
                            }))),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(px(12.))
                            .child(e.url.clone()),
                    )
                    .child(
                        row()
                            .gap_3()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(duration(e.duration_ms))
                            .child(e.size.map(bytes).unwrap_or("Unknown size".into()))
                            .child(e.kind.clone())
                            .child(
                                e.raw["response"]["httpVersion"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_owned(),
                            ),
                    ),
            )
            .child(
                row()
                    .h(px(40.))
                    .px_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .children(
                        [
                            (Tab::Headers, "Headers"),
                            (Tab::Payload, "Payload"),
                            (Tab::Response, "Response"),
                            (Tab::Cookies, "Cookies"),
                            (Tab::Timing, "Timing"),
                            (Tab::Raw, "Raw"),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(i, (tab, label))| {
                            div()
                                .id(("detail-tab", i))
                                .h_full()
                                .px_2()
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_size(px(11.))
                                .text_color(rgb(if self.tab == tab { TEXT } else { MUTED }))
                                .border_b_2()
                                .border_color(rgb(if self.tab == tab { ACCENT } else { PANEL }))
                                .hover(|d| d.bg(rgb(0x23232b)))
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.tab = tab;
                                    this.refresh_detail();
                                    cx.notify();
                                }))
                        }),
                    ),
            )
            .child(
                row()
                    .h(px(40.))
                    .px_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(button("copy-url", "Copy URL").on_click(
                        cx.listener(|this, _, window, cx| this.copy_url(&CopyUrl, window, cx)),
                    ))
                    .child(button("copy-curl", "Copy cURL").on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(e) = this.selected() {
                                cx.write_to_clipboard(ClipboardItem::new_string(e.curl()));
                                this.notice =
                                    "cURL copied · May contain credentials; review before running"
                                        .into();
                                cx.notify();
                            }
                        },
                    )))
                    .child(button("copy-content", "Copy content").on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(e) = this.selected() {
                                let content = detail_content(e, this.tab);
                                match content {
                                    Ok(content) => {
                                        cx.write_to_clipboard(ClipboardItem::new_string(content));
                                        this.notice =
                                            "Content copied (full value, not truncated preview)"
                                                .into();
                                    }
                                    Err(error) => this.error = Some(error),
                                }
                                cx.notify();
                            }
                        },
                    ))),
            )
            .when(self.tab == Tab::Timing, |d| d.child(self.timing(e)))
            .when(self.tab != Tab::Timing, |d| {
                d.child(
                    uniform_list(
                        "detail-lines",
                        self.lines.len(),
                        cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                            range
                                .map(|i| {
                                    row()
                                        .h(px(23.))
                                        .px_3()
                                        .gap_3()
                                        .font_family(MONO_FONT)
                                        .text_size(px(11.))
                                        .child(
                                            div()
                                                .w(px(30.))
                                                .flex_shrink_0()
                                                .text_color(rgb(0x626272))
                                                .child((i + 1).to_string()),
                                        )
                                        .child(
                                            div()
                                                .whitespace_nowrap()
                                                .text_color(rgb(if this.lines[i].contains(':') {
                                                    0xb7cafa
                                                } else {
                                                    0xc8c8d5
                                                }))
                                                .child(this.lines[i].clone()),
                                        )
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(self.detail_scroll.clone())
                    .with_horizontal_sizing_behavior(
                        gpui::ListHorizontalSizingBehavior::Unconstrained,
                    )
                    .with_width_from_item(
                        self.lines
                            .iter()
                            .enumerate()
                            .max_by_key(|(_, line)| line.len())
                            .map(|(i, _)| i),
                    )
                    .flex_1()
                    .min_h_0()
                    .py_3(),
                )
            })
    }

    fn timing(&self, entry: &Entry) -> impl IntoElement {
        let phases = entry.phases();
        let total = phases.iter().map(|p| p.duration_ms).sum::<f64>().max(1.);
        col().id("timings").overflow_y_scroll().flex_1().p_5().gap_4()
            .child(div().text_color(rgb(MUTED)).child("Connection lifecycle"))
            .children(phases.into_iter().map(|p| col().gap_2()
                .child(row().justify_between().child(p.name).child(duration(p.duration_ms)))
                .child(div().h(px(7.)).w_full().bg(rgb(BORDER)).rounded_sm().child(div().h_full().w(relative((p.duration_ms/total) as f32)).bg(rgb(phase_color(p.name))).rounded_sm()))))
            .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child("Connect excludes TLS here. HAR includes TLS in connect; the phases are not double-counted."))
    }

    fn overview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.session.as_ref().unwrap();
        let summary = &s.summary;
        let mut domains = BTreeMap::<String, (usize, u64)>::new();
        for &id in &s.visible {
            let e = &s.capture.entries[id];
            let item = domains.entry(e.host.clone()).or_default();
            item.0 += 1;
            item.1 += e.size.unwrap_or(0);
        }
        col().id("overview").flex_1().min_w_0().overflow_y_scroll().p_8().gap_6()
            .child(col().gap_2().child(div().text_size(px(24.)).child("Capture overview"))
                .child(div().text_color(rgb(MUTED)).child("A quieter view of your network. Metrics reflect the current filters.")))
            .child(row().gap_4().children([
                ("REQUESTS", summary.requests.to_string()), ("RESOURCE SIZE", bytes(summary.bytes)),
                ("WALL TIME", duration(summary.duration_ms)), ("ERRORS", summary.errors.to_string()),
            ].into_iter().map(|(label, value)| col().flex_1().p_5().gap_3().rounded_lg().bg(rgb(PANEL)).border_1().border_color(rgb(BORDER))
                .child(div().text_size(px(10.)).text_color(rgb(MUTED)).child(label))
                .child(div().text_size(px(26.)).child(value)))))
            .child(row().justify_between().child("Requests by domain").child(div().text_color(rgb(MUTED)).child(format!("Average request: {}", duration(summary.average_ms)))))
            .child(col().rounded_lg().border_1().border_color(rgb(BORDER)).children(domains.into_iter().map(|(host, (count, size))| {
                row().p_4().gap_4().border_b_1().border_color(rgb(BORDER)).child(div().w(px(260.)).child(host))
                    .child(div().flex_1().h(px(7.)).bg(rgb(BORDER)).rounded_sm().child(div().h_full().w(relative(count as f32/summary.requests.max(1) as f32)).bg(rgb(ACCENT)).rounded_sm()))
                    .child(div().w(px(90.)).child(format!("{count} requests"))).child(div().w(px(90.)).child(bytes(size)))
            })))
            .child(row().gap_3().child(button("copy-har", "Copy filtered HAR").on_click(cx.listener(|this, _, window, cx| this.copy_har(&CopyHar, window, cx))))
                .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child("Exports retain original headers, bodies and extension fields. Not sanitized.")))
    }

    fn issues(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.session.as_ref().unwrap();
        let findings: Vec<_> = s
            .visible
            .iter()
            .flat_map(|&id| {
                s.capture.entries[id]
                    .issues()
                    .into_iter()
                    .map(move |message| (id, message))
            })
            .collect();
        col().id("issues").flex_1().overflow_y_scroll().p_8().gap_4()
            .child(div().text_size(px(24.)).child(format!("{} findings", findings.len())))
            .child(div().text_color(rgb(MUTED)).child("Local checks for failures, slow responses, large assets and missing cache headers. Current filters apply."))
            .when(findings.is_empty(), |d| d.child(div().p_5().text_color(rgb(0x7ddbb1)).child("No issues detected by these checks.")))
            .children(findings.into_iter().enumerate().map(|(index, (id, message))| {
                col().id(("issue", index)).p_4().gap_2().rounded_md().bg(rgb(PANEL)).border_1().border_color(rgb(BORDER)).cursor_pointer().hover(|d| d.border_color(rgb(ACCENT)))
                    .child(div().text_color(rgb(0xebba80)).child(message))
                    .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(s.capture.entries[id].url.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| { if let Some(s) = &mut this.session { s.selected = Some(id); if let Some(index) = s.visible.iter().position(|&i| i == id) { this.list_scroll.scroll_to_item(index, ScrollStrategy::Center); } } this.view = View::Requests; this.inspector = true; this.refresh_detail(); cx.notify(); }))
            }))
            .when(!s.capture.warnings.is_empty(), |d| d.child(div().text_size(px(18.)).mt_4().child("Validation warnings"))
                .children(s.capture.warnings.iter().map(|message| div().text_color(rgb(0xebba80)).child(message.clone()))))
    }

    fn welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_5()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(ACCENT))
                    .child("NATIVE. FAST."),
            )
            .child(
                div()
                    .text_size(px(38.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Your network, in focus."),
            )
            .child(div().text_color(rgb(MUTED)).child(
                "Open a HAR capture to explore requests, responses and the time in between.",
            ))
            .child(
                col()
                    .w(px(580.))
                    .mt_4()
                    .p_8()
                    .gap_5()
                    .items_center()
                    .bg(rgb(PANEL))
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(div().text_size(px(18.)).child("Drop a .har file here"))
                    .child(
                        div()
                            .text_color(rgb(MUTED))
                            .child("or open a capture from your device"),
                    )
                    .child(
                        button("welcome-open", &format!("Open HAR file  {PRIMARY_LABEL} O"))
                            .bg(rgb(SELECTED))
                            .text_color(rgb(ACCENT))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open(&Open, window, cx)),
                            ),
                    )
                    .child(
                        button("sample", "Explore a sample capture →")
                            .on_click(cx.listener(|this, _, _, cx| this.sample(cx))),
                    ),
            )
            .child(
                div()
                    .mt_4()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("No account. No server. Nothing leaves your machine."),
            )
    }
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Removing a focused input must not leave keyboard actions attached to an
        // entity outside the dispatch tree (e.g. after asynchronous file loading).
        if (!self.open_path && self.path.focus_handle(cx).is_focused(window))
            || (!matches!(self.view, View::Requests | View::Waterfall)
                && self.search.focus_handle(cx).is_focused(window))
        {
            window.focus(&self.focus);
        }
        col().size_full().bg(rgb(BG)).text_color(rgb(TEXT)).font_family(UI_FONT).text_size(px(12.))
            .key_context("Viewer").track_focus(&self.focus)
            .on_action(cx.listener(Self::open)).on_action(cx.listener(Self::search_action))
            .on_action(cx.listener(Self::next)).on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::escape)).on_action(cx.listener(Self::submit)).on_action(cx.listener(Self::copy_url))
            .on_action(cx.listener(Self::copy_har))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| { if let Some(path) = paths.paths().first() { this.load(path.clone(), cx); } }))
            .child(self.header(cx))
            .when(self.open_path, |d| d.child(row().p_3().gap_3().bg(rgb(PANEL)).border_b_1().border_color(rgb(BORDER))
                .child(div().child("Open file"))
                .child(div().flex_1().min_w_0().child(self.path.clone()))
                .child(button("browse", "Browse…").on_click(cx.listener(|this, _, _, cx| this.browse(cx))))
                .child(button("load-path", "Open ↵").on_click(cx.listener(|this, _, window, cx| this.submit(&Submit, window, cx))))
                .child(button("cancel-open", "Cancel").on_click(cx.listener(|this, _, window, cx| this.escape(&Escape, window, cx))))))
            .when_some(self.error.clone(), |d, error| d.child(row().px_4().py_3().gap_3().bg(rgb(0x38232b)).text_color(rgb(0xffb6b6))
                .child(div().flex_1().child(error)).child(button("dismiss", "Dismiss").on_click(cx.listener(|this, _, _, cx| { this.error = None; cx.notify(); })))))
            .when(self.loading, |d| d.child(div().px_4().py_2().bg(rgb(SELECTED)).text_color(rgb(ACCENT)).child("Reading and parsing capture… You can keep inspecting the previous capture.")))
            .when(self.session.is_none(), |d| d.child(self.welcome(cx)))
            .when(self.session.is_some(), |d| d.child(self.navigation(cx)).child(
                div().flex().flex_1().min_h_0()
                    .child(self.sidebar(cx))
                    .when(matches!(self.view, View::Requests | View::Waterfall), |d| d.child(self.request_table(cx))
                        .when(self.inspector && self.selected().is_some() && self.view == View::Requests, |d| d.child(self.inspector(cx))))
                    .when(self.view == View::Overview, |d| d.child(self.overview(cx)))
                    .when(self.view == View::Issues, |d| d.child(self.issues(cx)))
            ))
            .child(row().h(px(30.)).flex_shrink_0().px_4().gap_5().border_t_1().border_color(rgb(BORDER)).bg(rgb(PANEL)).text_size(px(10.))
                .when_some(self.session.as_ref(), |d, s| d.child(format!("{} / {} requests", s.visible.len(), s.capture.entries.len()))
                    .child(format!("Resources {}", bytes(s.summary.bytes))).child(format!("Finish {}", duration(s.summary.duration_ms))))
                .child(div().flex_1().overflow_hidden().text_ellipsis().text_color(rgb(MUTED)).child(self.notice.clone()))
                .child(div().text_color(rgb(MUTED)).child(format!("↑ ↓ Navigate   ·   {PRIMARY_LABEL} F Search   ·   Esc Unfocus"))))
    }
}

impl Focusable for Viewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn row() -> gpui::Div {
    div().flex().items_center()
}
fn col() -> gpui::Div {
    div().flex().flex_col()
}
fn button(id: &'static str, text: &str) -> gpui::Stateful<gpui::Div> {
    row()
        .id(id)
        .px_3()
        .h(px(28.))
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .text_size(px(11.))
        .cursor_pointer()
        .hover(|d| d.bg(rgb(0x2c293b)).border_color(rgb(0x635a91)))
        .child(text.to_owned())
}
fn pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn detail_content(entry: &Entry, tab: Tab) -> Result<String, String> {
    Ok(match tab {
        Tab::Headers => {
            let mut text = format!(
                "{} {}\nStatus: {}\n\n",
                entry.method, entry.url, entry.status
            );
            for (response, title) in [(false, "REQUEST HEADERS"), (true, "RESPONSE HEADERS")] {
                text.push_str(&format!("{title}\n\n"));
                let headers = entry.headers(response);
                if headers.is_empty() {
                    text.push_str("No headers recorded\n");
                }
                for (name, value) in headers {
                    text.push_str(&format!("{name}: {value}\n"));
                }
                text.push('\n');
            }
            text
        }
        Tab::Payload => {
            let body = entry.body(false)?;
            if body.is_empty() && entry.raw["request"]["postData"]["params"].is_array() {
                pretty(&entry.raw["request"]["postData"]["params"])
            } else {
                body
            }
        }
        Tab::Response => entry.body(true)?,
        Tab::Cookies => format!(
            "REQUEST COOKIES\n\n{}\n\nRESPONSE COOKIES\n\n{}",
            pretty(&entry.raw["request"]["cookies"]),
            pretty(&entry.raw["response"]["cookies"])
        ),
        Tab::Timing => entry
            .phases()
            .iter()
            .map(|p| format!("{}: {}\n", p.name, duration(p.duration_ms)))
            .collect(),
        Tab::Raw => pretty(&entry.raw),
    })
}

fn bytes(size: u64) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1} MiB", size as f64 / (1024. * 1024.))
    } else if size >= 1024 {
        format!("{:.1} KiB", size as f64 / 1024.)
    } else {
        format!("{size} B")
    }
}
fn duration(ms: f64) -> String {
    if ms >= 1000. {
        format!("{:.2} s", ms / 1000.)
    } else {
        format!("{ms:.0} ms")
    }
}
fn status_color(status: u16) -> u32 {
    match status {
        100..=299 => 0x79dbaa,
        300..=399 => 0x8abff3,
        400..=499 => 0xe8b779,
        _ => 0xf08c96,
    }
}
fn phase_color(phase: &str) -> u32 {
    match phase {
        "dns" => 0x63bba4,
        "connect" => 0xe19d67,
        "ssl" => 0xa399ff,
        "send" => 0x8cc582,
        "wait" => 0x639edc,
        "receive" => 0xe4bc69,
        _ => 0x656573,
    }
}
