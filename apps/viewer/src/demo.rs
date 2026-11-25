use gpui_kit::component::button::Button;
use gpui_kit::prelude::FluentBuilder;
use gpui_nightshade_components::{
    NightshadeViewport,
    gpui_kit::{self, *},
};
struct Viewer {
    viewport: Entity<NightshadeViewport>,
    show_overlay: bool,
    second: Option<Entity<NightshadeViewport>>,
}
impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x171a21))
            .text_color(rgb(0xdce4f2))
            .child(
                div().flex().p_2().gap_4().child("Nightshade / GPUI").child(
                    Button::new("overlay")
                        .label("Toggle overlay")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_overlay = !this.show_overlay;
                            cx.notify();
                        })),
                ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .size_full()
                            .child(div().flex_1().h_full().child(self.viewport.clone()))
                            .when_some(self.second.clone(), |row, viewport| {
                                row.child(div().flex_1().h_full().child(viewport))
                            }),
                    )
                    .when(self.show_overlay, |container| {
                        container.child(
                            div()
                                .absolute()
                                .top_4()
                                .left_4()
                                .p_4()
                                .bg(rgb(0x35435c))
                                .rounded_md()
                                .child("GPUI overlay above the Nightshade viewport"),
                        )
                    }),
            )
    }
}
pub fn launch(backend: gpui_nightshade::presentation::BackendPreference, multiple: bool) {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                let viewport = cx.new(|cx| NightshadeViewport::with_backend(backend, cx));
                let second =
                    multiple.then(|| cx.new(|cx| NightshadeViewport::with_backend(backend, cx)));
                cx.new(|_| Viewer {
                    second,
                    viewport,
                    show_overlay: true,
                })
            })
            .expect("open GPUI window");
            cx.activate(true);
        });
}
