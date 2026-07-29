use gpui::{App, Application, Window, WindowOptions, div, prelude::*, rgb};

struct HelloWorld;

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .bg(rgb(0x18181b))
            .size_full()
            .justify_center()
            .items_center()
            .text_color(rgb(0xf4f4f5))
            .child("Hello World")
    }
}

pub fn run_ui() {
    Application::new().run(|cx: &mut App| {
        cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| HelloWorld))
            .unwrap();
    });
}
