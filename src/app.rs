use crate::platform::{PRIMARY_LABEL, PRIMARY_MODIFIER};
use crate::request_table::Requests;
use gpui::{
    App, ClipboardItem, Context, Entity, ExternalPaths, FocusHandle, Focusable, KeyBinding,
    PathPromptOptions, ScrollStrategy, Subscription, Window, actions, div, prelude::*, px, rgb,
};
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    alert::Alert,
    button::{Button, ButtonVariants},
    chart::BarChart,
    checkbox::Checkbox,
    description_list::DescriptionList,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    list::ListItem,
    progress::Progress,
    scroll::ScrollableElement,
    sidebar::{Sidebar, SidebarGroup, SidebarMenu, SidebarMenuItem},
    tab::{Tab as ComponentTab, TabBar},
    table::{Table, TableEvent, TableState},
    tag::Tag,
    v_flex,
};
use har_viewer::{
    har::{Capture, Entry},
    session::Session,
};
use std::{collections::BTreeMap, io::Read, path::PathBuf};

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
const VIEWS: [(View, &str); 4] = [
    (View::Requests, "Requests"),
    (View::Waterfall, "Waterfall"),
    (View::Overview, "Overview"),
    (View::Issues, "Issues"),
];
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Headers,
    Payload,
    Response,
    Cookies,
    Timing,
    Raw,
}
const TABS: [(Tab, &str); 6] = [
    (Tab::Headers, "Headers"),
    (Tab::Payload, "Payload"),
    (Tab::Response, "Response"),
    (Tab::Cookies, "Cookies"),
    (Tab::Timing, "Timing"),
    (Tab::Raw, "Raw"),
];
#[derive(Clone, Copy)]
enum Facet {
    Status,
    Method,
    Kind,
    Host,
}

pub struct Viewer {
    session: Entity<Option<Session>>,
    table: Entity<TableState<Requests>>,
    filename: String,
    search: Entity<InputState>,
    path: Entity<InputState>,
    detail: Entity<InputState>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    view: View,
    tab: Tab,
    inspector: bool,
    open_path: bool,
    loading: bool,
    load_generation: u64,
    error: Option<String>,
    notice: String,
    reset_search: bool,
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(&format!("{PRIMARY_MODIFIER}-o"), Open, Some("Viewer")),
        KeyBinding::new(&format!("{PRIMARY_MODIFIER}-f"), Search, Some("Viewer")),
        // The editor's own Find action must not shadow request search.
        KeyBinding::new(
            &format!("{PRIMARY_MODIFIER}-f"),
            Search,
            Some("Viewer > Input"),
        ),
        KeyBinding::new("down", Next, Some("Viewer")),
        KeyBinding::new("up", Previous, Some("Viewer")),
        KeyBinding::new("escape", Escape, Some("Viewer")),
        KeyBinding::new("escape", Escape, Some("Viewer > Table")),
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
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let session = cx.new(|_| None::<Session>);
        let table = cx.new(|cx| {
            TableState::new(Requests::new(session.clone()), window, cx)
                .loop_selection(false)
                .col_selectable(false)
                .col_movable(false)
        });
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(format!("Filter URL, method or status…  {PRIMARY_LABEL} F"))
        });
        let path = cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/capture.har"));
        let detail = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .line_number(true)
                .soft_wrap(false)
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &search,
                window,
                |this, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        let query = input.read(cx).value().to_string();
                        this.session.update(cx, |session, cx| {
                            if let Some(s) = session {
                                s.filter.query = query;
                                s.refresh();
                                cx.notify();
                            }
                        });
                        this.scroll_top(cx);
                    }
                    InputEvent::PressEnter { .. } => window.focus(&this.focus),
                    _ => {}
                },
            ),
            cx.subscribe_in(&path, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit(&Submit, window, cx);
                }
            }),
            cx.observe_in(&session, window, |this, _, window, cx| {
                this.sync_table(cx);
                this.refresh_detail(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(&table, window, |this, _, event: &TableEvent, window, cx| {
                if let TableEvent::ColumnWidthsChanged(widths) = event {
                    this.table.update(cx, |table, _| {
                        table.delegate_mut().set_widths(widths);
                    });
                }
                if let TableEvent::SelectRow(index) = event {
                    let mut changed = false;
                    this.session.update(cx, |session, cx| {
                        if let Some(s) = session {
                            let selected = s.visible.get(*index).copied();
                            if s.selected != selected {
                                s.selected = selected;
                                changed = true;
                                cx.notify();
                            }
                        }
                    });
                    // Programmatic synchronization emits the same event. Only user
                    // selection should reopen an inspector that was explicitly closed.
                    if changed {
                        this.inspector = true;
                    }
                    if this.table.focus_handle(cx).is_focused(window) {
                        this.inspector = true;
                    }
                    cx.notify();
                }
            }),
        ];
        Self {
            session,
            table,
            filename: String::new(),
            search,
            path,
            detail,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
            view: View::Requests,
            tab: Tab::Headers,
            inspector: true,
            open_path: false,
            loading: false,
            load_generation: 0,
            error: None,
            notice: "Local only. No uploads, telemetry or replay.".into(),
            reset_search: false,
        }
    }

    fn sync_table(&self, cx: &mut Context<Self>) {
        let selected = self.session.read(cx).as_ref().and_then(|s| {
            s.selected
                .and_then(|id| s.visible.iter().position(|&i| i == id))
        });
        self.table.update(cx, |table, cx| {
            table.delegate_mut().sync_sort(cx);
            table.refresh(cx);
            if table.selected_row() != selected {
                if let Some(index) = selected {
                    table.set_selected_row(index, cx);
                } else {
                    table.clear_selection(cx);
                }
            }
        });
    }

    fn scroll_top(&self, cx: &App) {
        self.table
            .read(cx)
            .vertical_scroll_handle
            .scroll_to_item(0, ScrollStrategy::Top);
    }

    fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        if (self.view == View::Waterfall) != (view == View::Waterfall) {
            self.table.update(cx, |table, cx| {
                table.delegate_mut().configure(view == View::Waterfall);
                table.delegate_mut().sync_sort(cx);
                table.refresh(cx);
            });
        }
        self.view = view;
        cx.notify();
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
        self.session.update(cx, |session, cx| {
            *session = Some(Session::new(capture));
            cx.notify();
        });
        self.filename = filename;
        self.loading = false;
        self.open_path = false;
        self.error = None;
        self.set_view(View::Requests, cx);
        self.inspector = true;
        self.reset_search = true;
        self.scroll_top(cx);
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
        self.path.update(cx, |input, cx| input.focus(window, cx));
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
            let path = self.path.read(cx).value().to_string();
            if !path.is_empty() {
                self.load(PathBuf::from(path), cx);
            }
        } else {
            window.focus(&self.focus);
        }
    }

    fn search_action(&mut self, _: &Search, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.read(cx).is_some() {
            self.set_view(View::Requests, cx);
            self.search.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        self.open_path = false;
        self.error = None;
        window.focus(&self.focus);
        cx.notify();
    }

    fn next(&mut self, _: &Next, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus.is_focused(window) {
            self.navigate(1, cx);
        }
    }
    fn previous(&mut self, _: &Previous, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus.is_focused(window) {
            self.navigate(-1, cx);
        }
    }
    fn navigate(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            if let Some(s) = session {
                s.navigate(delta);
                cx.notify();
            }
        });
        self.inspector = true;
    }

    fn selected<'a>(&'a self, cx: &'a App) -> Option<&'a Entry> {
        let s = self.session.read(cx).as_ref()?;
        s.capture.entries.get(s.selected?)
    }

    fn copy_url(&mut self, _: &CopyUrl, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(e) = self.selected(cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(e.url.clone()));
            self.notice = "URL copied".into();
            cx.notify();
        }
    }

    fn copy_har(&mut self, _: &CopyHar, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.session.read(cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(s.capture.export(&s.visible)));
            self.notice = "HAR copied · Credentials are NOT redacted; review before sharing".into();
            cx.notify();
        }
    }

    fn refresh_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self
            .selected(cx)
            .map(|e| detail_content(e, self.tab).unwrap_or_else(|error| error))
            .unwrap_or_default();
        let text = if text.is_empty() {
            "No body recorded in this HAR.".into()
        } else {
            text
        };
        let mut lines: Vec<String> = text
            .lines()
            .take(5000)
            .map(|line| line.chars().take(2000).collect())
            .collect();
        if text.lines().count() > 5000 || text.lines().any(|l| l.chars().count() > 2000) {
            lines.insert(0, "[Preview limited to 5,000 lines / 2,000 characters per line. Copy for full content.]".into());
        }
        self.detail.update(cx, |input, cx| {
            input.set_value(lines.join("\n"), window, cx)
        });
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            if let Some(s) = session {
                s.filter = Default::default();
                s.refresh();
                cx.notify();
            }
        });
        self.reset_search = true;
        self.scroll_top(cx);
        cx.notify();
    }

    fn toggle_facet(&mut self, facet: Facet, value: &str, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            if let Some(s) = session {
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
                cx.notify();
            }
        });
        self.scroll_top(cx);
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.session.read(cx) else {
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
        h_flex()
            .h(px(60.))
            .px_4()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("HAR Viewer"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.filename.clone()),
            )
            .child(Tag::success().child("Local only"))
            .child(
                Button::new("open")
                    .label(format!("Open HAR  {PRIMARY_LABEL} O"))
                    .icon(IconName::FolderOpen)
                    .on_click(cx.listener(|this, _, window, cx| this.open(&Open, window, cx))),
            )
            .when(self.session.read(cx).is_some(), |d| {
                d.child(
                    Button::new("export")
                        .label("Export HAR")
                        .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                )
            })
    }

    fn navigation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        TabBar::new("views")
            .selected_index(self.view as usize)
            .children(VIEWS.map(|(_, label)| ComponentTab::new().label(label)))
            .on_click(cx.listener(|this, index: &usize, _, cx| this.set_view(VIEWS[*index].0, cx)))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.session.read(cx).as_ref().unwrap();
        let groups = [
            (Facet::Status, "Status"),
            (Facet::Method, "Method"),
            (Facet::Kind, "Resource type"),
            (Facet::Host, "Domain"),
        ]
        .into_iter()
        .map(|(facet, title)| {
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
            SidebarGroup::new(title).child(SidebarMenu::new().gap_1().children(
                counts.into_iter().map(|(key, count)| {
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
                    SidebarMenuItem::new(label)
                        .active(active)
                        .icon(if active {
                            IconName::Check
                        } else {
                            IconName::Plus
                        })
                        .suffix(div().text_xs().child(count.to_string()))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.toggle_facet(facet, &key, cx)),
                        )
                }),
            ))
        })
        .collect::<Vec<_>>();
        Sidebar::left()
            .w(px(230.))
            .collapsible(false)
            .header(
                h_flex().w_full().justify_between().child("Filters").child(
                    Button::new("reset")
                        .small()
                        .ghost()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                ),
            )
            .children(groups)
            .footer(
                Checkbox::new("slow")
                    .label("Only slow ≥ 1 s")
                    .checked(s.filter.slow_only)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.session.update(cx, |session, cx| {
                            if let Some(s) = session {
                                s.filter.slow_only = *checked;
                                s.refresh();
                                cx.notify();
                            }
                        });
                        this.scroll_top(cx);
                    })),
            )
    }

    fn histogram(&self, cx: &App) -> impl IntoElement {
        let s = self.session.read(cx).as_ref().unwrap();
        let mut bins = [(0u32, 0u32); 64];
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
        let normal = cx.theme().chart_1;
        let error = cx.theme().danger;
        v_flex()
            .px_3()
            .py_2()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Requests over time")
                    .child(format!("0 — {}", duration(span))),
            )
            .child(
                div().h(px(64.)).child(
                    BarChart::new(bins.into_iter().enumerate().collect::<Vec<_>>())
                        .x(|(i, _)| i.to_string())
                        .y(|(_, (count, _))| f64::from(*count))
                        .tick_margin(64)
                        .fill(move |(_, (_, errors))| if *errors > 0 { error } else { normal }),
                ),
            )
    }

    fn request_table(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.session.read(cx).as_ref().unwrap();
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                h_flex()
                    .h(px(52.))
                    .px_3()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.search).small()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .child(format!("{} requests", s.visible.len())),
                    ),
            )
            .child(self.histogram(cx))
            .when(self.view == View::Waterfall, |d| {
                d.child(
                    h_flex().px_4().h(px(30.)).gap_3().text_xs().children(
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
            .when(s.visible.is_empty(), |d| {
                d.child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child("No requests match these filters")
                        .child(
                            Button::new("clear-empty")
                                .label("Clear filters")
                                .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                        ),
                )
            })
            .when(!s.visible.is_empty(), |d| {
                d.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(Table::new(&self.table).large().bordered(false).stripe(true)),
                )
            })
    }

    fn inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let e = self.selected(cx).unwrap();
        v_flex()
            .w(px(480.))
            .flex_shrink_0()
            .min_h_0()
            .border_l_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .p_3()
                    .gap_3()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Tag::secondary().child(e.method.clone()))
                            .child(format!(
                                "{} {}",
                                e.status,
                                e.raw["response"]["statusText"].as_str().unwrap_or("")
                            ))
                            .child(div().flex_1())
                            .child(
                                Button::new("previous")
                                    .small()
                                    .ghost()
                                    .icon(IconName::ArrowUp)
                                    .tooltip("Previous request")
                                    .on_click(cx.listener(|this, _, _, cx| this.navigate(-1, cx))),
                            )
                            .child(
                                Button::new("next")
                                    .small()
                                    .ghost()
                                    .icon(IconName::ArrowDown)
                                    .tooltip("Next request")
                                    .on_click(cx.listener(|this, _, _, cx| this.navigate(1, cx))),
                            )
                            .child(
                                Button::new("close")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Close)
                                    .tooltip("Close inspector")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.inspector = false;
                                        window.focus(&this.focus);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(div().text_sm().text_ellipsis().child(e.url.clone()))
                    .child(
                        h_flex()
                            .gap_3()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
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
                TabBar::new("detail-tabs")
                    .small()
                    .selected_index(self.tab as usize)
                    .children(TABS.map(|(_, label)| ComponentTab::new().label(label)))
                    .on_click(cx.listener(|this, index: &usize, window, cx| {
                        this.tab = TABS[*index].0;
                        this.refresh_detail(window, cx);
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .h(px(44.))
                    .px_3()
                    .gap_2()
                    .child(Button::new("copy-url").small().label("Copy URL").on_click(
                        cx.listener(|this, _, window, cx| this.copy_url(&CopyUrl, window, cx)),
                    ))
                    .child(
                        Button::new("copy-curl")
                            .small()
                            .label("Copy cURL")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(e) = this.selected(cx) {
                                    cx.write_to_clipboard(ClipboardItem::new_string(e.curl()));
                                    this.notice =
                                    "cURL copied · May contain credentials; review before running"
                                        .into();
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("copy-content")
                            .small()
                            .label("Copy content")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(e) = this.selected(cx) {
                                    match detail_content(e, this.tab) {
                                        Ok(content) => {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                content,
                                            ));
                                            this.notice =
                                            "Content copied (full value, not truncated preview)"
                                                .into();
                                        }
                                        Err(error) => this.error = Some(error),
                                    }
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .when(self.tab == Tab::Timing, |d| d.child(self.timing(e, cx)))
            .when(self.tab != Tab::Timing, |d| {
                d.child(
                    div().flex_1().min_h_0().child(
                        Input::new(&self.detail)
                            .h_full()
                            .disabled(true)
                            .bordered(false),
                    ),
                )
            })
    }

    fn timing(&self, entry: &Entry, cx: &App) -> impl IntoElement {
        let phases = entry.phases();
        let total = phases.iter().map(|p| p.duration_ms).sum::<f64>().max(1.);
        let content = v_flex().p_5().gap_4()
            .child("Connection lifecycle")
            .children(phases.into_iter().map(|p| v_flex().gap_2()
                .child(h_flex().justify_between().child(p.name).child(duration(p.duration_ms)))
                .child(Progress::new().value((p.duration_ms / total * 100.) as f32).bg(rgb(phase_color(p.name))))))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Connect excludes TLS here. HAR includes TLS in connect; the phases are not double-counted."));
        div()
            .id("timings")
            .flex_1()
            .min_h_0()
            .child(content)
            .overflow_y_scrollbar()
    }

    fn overview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.session.read(cx).as_ref().unwrap();
        let summary = &s.summary;
        let mut domains = BTreeMap::<String, (usize, u64)>::new();
        for &id in &s.visible {
            let e = &s.capture.entries[id];
            let item = domains.entry(e.host.clone()).or_default();
            item.0 += 1;
            item.1 += e.size.unwrap_or(0);
        }
        let content = v_flex()
            .p_6()
            .gap_5()
            .child(div().text_2xl().child("Capture overview"))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Metrics reflect the current filters."),
            )
            .child(
                DescriptionList::vertical()
                    .columns(4)
                    .item("Requests", summary.requests.to_string(), 1)
                    .item("Resource size", bytes(summary.bytes), 1)
                    .item("Wall time", duration(summary.duration_ms), 1)
                    .item("Errors", summary.errors.to_string(), 1),
            )
            .child(
                GroupBox::new().outline().title("Requests by domain").child(
                    v_flex()
                        .gap_4()
                        .children(domains.into_iter().map(|(host, (count, size))| {
                            h_flex()
                                .gap_4()
                                .child(div().w(px(260.)).child(host))
                                .child(div().flex_1().child(
                                    Progress::new().value(
                                        count as f32 / summary.requests.max(1) as f32 * 100.,
                                    ),
                                ))
                                .child(div().w(px(110.)).child(format!("{count} requests")))
                                .child(div().w(px(90.)).child(bytes(size)))
                        })),
                ),
            )
            .child(format!("Average request: {}", duration(summary.average_ms)))
            .child(
                Button::new("copy-har").label("Copy filtered HAR").on_click(
                    cx.listener(|this, _, window, cx| this.copy_har(&CopyHar, window, cx)),
                ),
            )
            .child(
                Alert::warning(
                    "export-warning",
                    "Exports retain original headers, bodies and extension fields. Not sanitized.",
                )
                .text_color(cx.theme().foreground),
            );
        div()
            .id("overview")
            .flex_1()
            .min_w_0()
            .child(content)
            .overflow_y_scrollbar()
    }

    fn issues(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.session.read(cx).as_ref().unwrap();
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
        let content = v_flex().p_6().gap_4()
            .child(div().text_2xl().child(format!("{} findings", findings.len())))
            .child(div().text_color(cx.theme().muted_foreground).child("Local checks for failures, slow responses, large assets and missing cache headers. Current filters apply."))
            .when(findings.is_empty(), |d| d.child(Alert::success("no-issues", "No issues detected by these checks.").text_color(cx.theme().foreground)))
            .children(findings.into_iter().enumerate().map(|(index, (id, message))| ListItem::new(("issue", index))
                .child(v_flex().gap_2().py_2().child(div().child(message))
                    .child(div().text_xs().child(s.capture.entries[id].url.clone())))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.session.update(cx, |session, cx| { if let Some(s) = session { s.selected = Some(id); cx.notify(); } });
                    this.set_view(View::Requests, cx); this.inspector = true; window.focus(&this.focus);
                }))))
            .when(!s.capture.warnings.is_empty(), |d| d.child(div().text_lg().child("Validation warnings"))
                .children(s.capture.warnings.iter().enumerate().map(|(i, message)| Alert::warning(("validation", i), message.clone()).text_color(cx.theme().foreground))));
        div()
            .id("issues")
            .flex_1()
            .min_w_0()
            .child(content)
            .overflow_y_scrollbar()
    }

    fn welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_5()
            .child(div().text_3xl().child("Inspect your HTTP archives"))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("No account. No server. Nothing leaves your machine."),
            )
            .child(
                div().w(px(560.)).child(
                    GroupBox::new()
                        .outline()
                        .title("Drop a .har file here")
                        .child(
                        v_flex()
                            .gap_4()
                            .child(
                                "Open a capture from your device, or explore the fictional sample.",
                            )
                            .child(
                                Button::new("welcome-open")
                                    .primary()
                                    .label(format!("Open HAR file  {PRIMARY_LABEL} O"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open(&Open, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("sample")
                                    .label("Explore a sample capture")
                                    .on_click(cx.listener(|this, _, _, cx| this.sample(cx))),
                            ),
                    ),
                ),
            )
    }
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.reset_search {
            self.reset_search = false;
            self.search
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        // Keep shortcuts reachable when a focused field leaves the dispatch tree.
        if (!self.open_path && self.path.focus_handle(cx).is_focused(window))
            || (!matches!(self.view, View::Requests | View::Waterfall)
                && self.search.focus_handle(cx).is_focused(window))
            || ((!matches!(self.view, View::Requests | View::Waterfall)
                || self
                    .session
                    .read(cx)
                    .as_ref()
                    .is_none_or(|s| s.visible.is_empty()))
                && self.table.focus_handle(cx).is_focused(window))
            || ((!self.inspector || self.view != View::Requests || self.tab == Tab::Timing)
                && self.detail.focus_handle(cx).is_focused(window))
        {
            window.focus(&self.focus);
        }
        v_flex().size_full().bg(cx.theme().background).text_color(cx.theme().foreground).text_sm()
            .key_context("Viewer").track_focus(&self.focus)
            .on_action(cx.listener(Self::open)).on_action(cx.listener(Self::search_action))
            .on_action(cx.listener(Self::next)).on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::escape)).on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::copy_url)).on_action(cx.listener(Self::copy_har))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| { if let Some(path) = paths.paths().first() { this.load(path.clone(), cx); } }))
            .child(self.header(cx))
            .when(self.open_path, |d| d.child(h_flex().p_3().gap_3().border_b_1().border_color(cx.theme().border)
                .child("Open file").child(div().flex_1().min_w_0().child(Input::new(&self.path)))
                .child(Button::new("browse").label("Browse…").on_click(cx.listener(|this, _, _, cx| this.browse(cx))))
                .child(Button::new("load-path").primary().label("Open ↵").on_click(cx.listener(|this, _, window, cx| this.submit(&Submit, window, cx))))
                .child(Button::new("cancel-open").label("Cancel").on_click(cx.listener(|this, _, window, cx| this.escape(&Escape, window, cx))))))
            .when_some(self.error.clone(), |d, error| d.child(Alert::error("load-error", error).banner().text_color(cx.theme().foreground)
                .on_close(cx.listener(|this, _, _, cx| { this.error = None; cx.notify(); }))))
            .when(self.loading, |d| d.child(Alert::info("loading", "Reading and parsing capture… You can keep inspecting the previous capture.").banner().text_color(cx.theme().foreground)))
            .when(self.session.read(cx).is_none(), |d| d.child(self.welcome(cx)))
            .when(self.session.read(cx).is_some(), |d| d.child(self.navigation(cx)).child(div().flex().flex_1().min_h_0()
                .child(self.sidebar(cx))
                .when(matches!(self.view, View::Requests | View::Waterfall), |d| d.child(self.request_table(cx))
                    .when(self.inspector && self.selected(cx).is_some() && self.view == View::Requests, |d| d.child(self.inspector(cx))))
                .when(self.view == View::Overview, |d| d.child(self.overview(cx)))
                .when(self.view == View::Issues, |d| d.child(self.issues(cx)))))
            .child(h_flex().h(px(32.)).flex_shrink_0().px_4().gap_4().border_t_1().border_color(cx.theme().border).text_xs()
                .when_some(self.session.read(cx).as_ref(), |d, s| d.child(format!("{} / {} requests", s.visible.len(), s.capture.entries.len()))
                    .child(format!("Resources {}", bytes(s.summary.bytes))).child(format!("Finish {}", duration(s.summary.duration_ms))))
                .child(div().flex_1().min_w_0().text_ellipsis().text_color(cx.theme().muted_foreground).child(self.notice.clone())))
    }
}

impl Focusable for Viewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
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

pub(crate) fn bytes(size: u64) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1} MiB", size as f64 / (1024. * 1024.))
    } else if size >= 1024 {
        format!("{:.1} KiB", size as f64 / 1024.)
    } else {
        format!("{size} B")
    }
}
pub(crate) fn duration(ms: f64) -> String {
    if ms >= 1000. {
        format!("{:.2} s", ms / 1000.)
    } else {
        format!("{ms:.0} ms")
    }
}
pub(crate) fn phase_color(phase: &str) -> u32 {
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
