use crate::worker::{ViewRequest, Worker};
use gpui_kit::*;
use std::{sync::Arc, time::Duration};

pub struct NightshadeViewport {
    worker: Worker,
    image: Option<Arc<RenderImage>>,
    diagnostics: String,
    request: ViewRequest,
    last_pointer: Option<Point<Pixels>>,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    scale: f32,
    pick_sequence: u64,
    _poll: Task<()>,
}
impl NightshadeViewport {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::with_backend(gpui_nightshade::presentation::BackendPreference::Auto, cx)
    }
    pub fn with_backend(
        backend: gpui_nightshade::presentation::BackendPreference,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::from_factory(backend, Arc::new(gpui_nightshade::host::cube_viewport), cx)
    }
    /// Creates a viewport whose scene is built entirely on the render worker.
    pub fn from_factory(
        backend: gpui_nightshade::presentation::BackendPreference,
        factory: crate::SceneFactory,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|view, cx| {
            if let Some(image) = view.image.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        cx.on_app_quit(|view, cx| {
            let thread = view.worker.shutdown();
            cx.background_executor().spawn(async move {
                if let Some(thread) = thread {
                    let _ = thread.join();
                }
            })
        })
        .detach();
        let poll = cx.spawn(async |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                if view
                    .update(cx, |view, cx| {
                        if view.worker.has_update() {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            worker: Worker::new(backend, factory),
            image: None,
            diagnostics: "Initializing".into(),
            request: ViewRequest {
                width: 640,
                height: 480,
                yaw: 0.6,
                pitch: 0.35,
                distance: 4.,
                target: [0.; 3],
                pick: None,
            },
            last_pointer: None,
            bounds: Bounds::default(),
            scale: 1.,
            pick_sequence: 0,
            focus: cx.focus_handle(),
            _poll: poll,
        }
    }
}
impl Focusable for NightshadeViewport {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for NightshadeViewport {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (image, diagnostics) = self.worker.take();
        if let Some(image) = image
            && let Some(old) = self.image.replace(image)
        {
            let _ = window.drop_image(old);
        }
        self.diagnostics = diagnostics;
        let image = self.image.clone();
        let entity = cx.entity().downgrade();
        let paint_entity = entity.clone();
        div()
            .id("nightshade-viewport")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    if let Some(point) = gpui_nightshade::input::local_pointer(
                        [event.position.x.into(), event.position.y.into()],
                        [this.bounds.origin.x.into(), this.bounds.origin.y.into()],
                        [
                            this.bounds.size.width.into(),
                            this.bounds.size.height.into(),
                        ],
                        this.scale,
                        false,
                    ) {
                        this.pick_sequence += 1;
                        this.request.pick =
                            Some((point[0] as u32, point[1] as u32, this.pick_sequence));
                        this.worker.request(this.request);
                    }
                    this.last_pointer = Some(event.position);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.last_pointer = None),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.last_pointer = None),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if let Some(last) = this.last_pointer.replace(event.position) {
                    if event.dragging() {
                        if event.modifiers.shift {
                            this.request.target[0] -= f32::from(event.position.x - last.x)
                                * this.request.distance
                                * 0.002;
                            this.request.target[1] += f32::from(event.position.y - last.y)
                                * this.request.distance
                                * 0.002;
                        } else {
                            this.request.yaw -= f32::from(event.position.x - last.x) * 0.01;
                            this.request.pitch = (this.request.pitch
                                + f32::from(event.position.y - last.y) * 0.01)
                                .clamp(-1.5, 1.5);
                        }
                        this.worker.request(this.request);
                        cx.notify();
                    } else {
                        this.last_pointer = None;
                    }
                }
            }))
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(16.)).y;
                this.request.distance =
                    (this.request.distance * (-f32::from(delta) * 0.003).exp()).clamp(0.5, 100.);
                this.worker.request(this.request);
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let scale = window.scale_factor();
                        let _ = entity.update(cx, |this, _| {
                            this.bounds = bounds;
                            this.scale = scale;
                            this.request.width =
                                (f32::from(bounds.size.width) * scale).round().max(1.) as u32;
                            this.request.height =
                                (f32::from(bounds.size.height) * scale).round().max(1.) as u32;
                            this.worker.request(this.request);
                        });
                    },
                    move |bounds, (), window, cx| {
                        if let Some(image) = image
                            && let Err(error) = window.paint_image(
                                bounds,
                                bounds,
                                Corners::default(),
                                image,
                                0,
                                false,
                            )
                        {
                            let _ = paint_entity.update(cx, |view, cx| {
                                view.image = None;
                                view.worker.paint_failed(format!("{error:#}"));
                                cx.notify();
                            });
                        }
                    },
                )
                .w_full()
                .flex_1(),
            )
            .child(div().text_xs().p_1().child(self.diagnostics.clone()))
    }
}
