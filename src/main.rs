mod app;
mod platform;
mod request_table;

use gpui::{
    App, Application, Bounds, Focusable, TitlebarOptions, WindowBounds, WindowOptions, prelude::*,
    px, size,
};
use gpui_component::{Root, Theme, ThemeConfig, ThemeMode};
use gpui_component_assets::Assets;

const USAGE: &str = "Usage: har-viewer --input <har file>";

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--help" || a == "-h") {
        println!(
            "HAR Viewer\n\n{USAGE}\n\nNative HAR inspector. Requires macOS with Metal, or a Linux X11/Wayland desktop with Vulkan.\nCaptures can only be loaded at launch with --input.\n{0} F: search · Up/Down: navigate · {0} Q: quit",
            platform::PRIMARY_LABEL
        );
        return;
    }
    let input = match args.as_slice() {
        [option, path]
            if option == "--input"
                && !path.is_empty()
                && !path.to_string_lossy().starts_with('-') =>
        {
            path.clone()
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("HAR Viewer needs an X11 or Wayland desktop. Run it from your desktop session.");
        std::process::exit(1);
    }
    Application::new().with_assets(Assets).run(move |cx: &mut App| {
        gpui_component::init(cx);
        let gruvbox: ThemeConfig =
            serde_json::from_str(include_str!("../assets/gruvbox-dark.json"))
                .expect("embedded Gruvbox Dark theme must be valid");
        Theme::global_mut(cx).dark_theme = std::rc::Rc::new(gruvbox);
        Theme::change(ThemeMode::Dark, None, cx);
        let theme = Theme::global_mut(cx);
        theme.font_family = platform::UI_FONT.into();
        theme.mono_font_family = platform::MONO_FONT.into();
        app::init(cx);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(1100.), px(650.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("HAR Viewer".into()),
                    ..Default::default()
                }),
                app_id: Some("dev.thekorn.har-viewer".into()),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| {
                    let mut viewer = app::Viewer::new(window, cx);
                    viewer.load((&input).into(), cx);
                    viewer
                });
                window.focus(&view.focus_handle(cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        if let Err(error) = result {
            eprintln!(
                "Could not open HAR Viewer: {error}. Check your desktop session and graphics support (Metal on macOS, Vulkan on Linux)."
            );
            cx.quit();
        }
        cx.activate(true);
    });
}
