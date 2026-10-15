mod app;
mod input;
mod platform;

use gpui::{
    App, Application, Bounds, Focusable, TitlebarOptions, WindowBounds, WindowOptions, prelude::*,
    px, size,
};

const USAGE: &str = "Usage: har-viewer [--input <har file> | --sample]";

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--help" || a == "-h") {
        println!(
            "HAR Viewer\n\n{USAGE}\n\nNative HAR inspector. Requires macOS with Metal, or a Linux X11/Wayland desktop with Vulkan.\n{0} O: open file · {0} F: search · Up/Down: navigate · {0} Q: quit",
            platform::PRIMARY_LABEL
        );
        return;
    }
    let input = match args.as_slice() {
        [] => None,
        [arg] if arg == "--sample" => Some(arg.clone()),
        [option, path]
            if option == "--input"
                && !path.is_empty()
                && !path.to_string_lossy().starts_with('-') =>
        {
            Some(path.clone())
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
    Application::new().run(move |cx: &mut App| {
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
                    let mut viewer = app::Viewer::new(cx);
                    if let Some(arg) = &input {
                        if arg == "--sample" {
                            viewer.sample(cx);
                        } else {
                            viewer.load(arg.into(), cx);
                        }
                    }
                    viewer
                });
                window.focus(&view.focus_handle(cx));
                view
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
