use gpui::{App, Application, Context, Window, WindowOptions, div, prelude::*, rgb};

struct Viewer;

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x111114))
            .text_color(rgb(0xeeeeee))
            .child("HAR Viewer")
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Viewer))
            .expect("Could not open a desktop window");
        cx.activate(true);
    });
}
