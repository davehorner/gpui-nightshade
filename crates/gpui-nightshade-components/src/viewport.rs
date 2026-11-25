use crate::worker::{ViewRequest, Worker};
use gpui_kit::*;
use std::{sync::Arc, time::Duration};

pub struct NightshadeViewport {
    worker: Worker,
    image: Option<Arc<RenderImage>>,
    diagnostics: String,
    request: ViewRequest,
    last_pointer: Option<Point<Pixels>>,
    drag_button: Option<MouseButton>,
    scene_input: bool,
    input_subscriptions: Option<Vec<Subscription>>,
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
            drag_button: None,
            scene_input: false,
            input_subscriptions: None,
            bounds: Bounds::default(),
            scale: 1.,
            pick_sequence: 0,
            focus: cx.focus_handle(),
            _poll: poll,
        }
    }
    /// Use trunk scene input for left-button picking/gizmos; right-drag navigates.
    pub fn with_scene_input(mut self) -> Self {
        self.scene_input = true;
        self
    }
    fn local_position(&self, position: Point<Pixels>, captured: bool) -> Option<[f32; 2]> {
        gpui_nightshade::input::local_pointer(
            [position.x.into(), position.y.into()],
            [self.bounds.origin.x.into(), self.bounds.origin.y.into()],
            [
                self.bounds.size.width.into(),
                self.bounds.size.height.into(),
            ],
            self.scale,
            captured,
        )
    }
    fn scene_event(
        &self,
        phase: gpui_nightshade::input::PointerPhase,
        position: Point<Pixels>,
        modifiers: Modifiers,
        captured: bool,
    ) {
        if !self.scene_input {
            return;
        }
        if let Some(position) = self.local_position(position, captured) {
            self.worker.input(gpui_nightshade::input::PointerInput {
                phase,
                position,
                control: modifiers.control,
                shift: modifiers.shift,
            });
        } else if phase == gpui_nightshade::input::PointerPhase::Move {
            self.worker.input(gpui_nightshade::input::PointerInput {
                phase: gpui_nightshade::input::PointerPhase::Leave,
                position: [0.; 2],
                control: false,
                shift: false,
            });
        }
    }
    fn cancel_input(&mut self) {
        if self.scene_input {
            self.worker.cancel_input();
        }
        self.drag_button = None;
        self.last_pointer = None;
    }
    fn move_pointer(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.drag_button.is_some() && !event.dragging() {
            self.cancel_input();
            return;
        }
        if let Some(button) = self.drag_button {
            if self.scene_input && button == MouseButton::Left {
                self.scene_event(
                    gpui_nightshade::input::PointerPhase::Move,
                    event.position,
                    event.modifiers,
                    true,
                );
            } else if let Some(last) = self.last_pointer {
                if event.modifiers.shift {
                    self.request.target[0] -=
                        f32::from(event.position.x - last.x) * self.request.distance * 0.002;
                    self.request.target[1] +=
                        f32::from(event.position.y - last.y) * self.request.distance * 0.002;
                } else {
                    self.request.yaw -= f32::from(event.position.x - last.x) * 0.01;
                    self.request.pitch = (self.request.pitch
                        + f32::from(event.position.y - last.y) * 0.01)
                        .clamp(-1.5, 1.5);
                }
                self.worker.request(self.request);
                cx.notify();
            }
            self.last_pointer = Some(event.position);
        } else if self.scene_input {
            self.scene_event(
                gpui_nightshade::input::PointerPhase::Move,
                event.position,
                event.modifiers,
                false,
            );
        }
    }
    fn release_pointer(&mut self, event: &MouseUpEvent) {
        if self.drag_button != Some(event.button) {
            return;
        }
        if self.scene_input && event.button == MouseButton::Left {
            self.scene_event(
                gpui_nightshade::input::PointerPhase::Release,
                event.position,
                event.modifiers,
                true,
            );
        }
        self.drag_button = None;
        self.last_pointer = None;
    }
}
impl Focusable for NightshadeViewport {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for NightshadeViewport {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.input_subscriptions.is_none() {
            let focus = self.focus.clone();
            self.input_subscriptions = Some(vec![
                cx.on_focus_out(&focus, window, |view, _, _, _| view.cancel_input()),
                cx.observe_window_activation(window, |view, window, _| {
                    if !window.is_window_active() {
                        view.cancel_input();
                    }
                }),
            ]);
        }
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
            .on_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                let camera_button = if this.scene_input {
                    MouseButton::Right
                } else {
                    MouseButton::Left
                };
                if event.button != MouseButton::Left && event.button != camera_button {
                    return;
                }
                let Some(point) = this.local_position(event.position, false) else {
                    return;
                };
                window.focus(&this.focus, cx);
                this.drag_button = Some(event.button);
                this.last_pointer = Some(event.position);
                if this.scene_input && event.button == MouseButton::Left {
                    this.scene_event(
                        gpui_nightshade::input::PointerPhase::Press,
                        event.position,
                        event.modifiers,
                        false,
                    );
                } else if !this.scene_input {
                    this.pick_sequence += 1;
                    this.request.pick =
                        Some((point[0] as u32, point[1] as u32, this.pick_sequence));
                    this.worker.request(this.request);
                } else {
                    this.worker.cancel_input();
                }
                cx.stop_propagation();
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.drag_button.is_none() {
                    this.move_pointer(event, cx);
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, _| {
                if !*hovered && this.drag_button.is_none() && this.scene_input {
                    this.worker.input(gpui_nightshade::input::PointerInput {
                        phase: gpui_nightshade::input::PointerPhase::Leave,
                        position: [0.; 2],
                        control: false,
                        shift: false,
                    });
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.cancel_input();
                    cx.stop_propagation();
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
                            if (this.bounds != bounds || this.scale != scale)
                                && this.drag_button.is_some()
                            {
                                this.cancel_input();
                            }
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
                        let move_entity = paint_entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = move_entity.update(cx, |view, cx| {
                                    if view.drag_button.is_some() {
                                        view.move_pointer(event, cx);
                                    }
                                });
                            }
                        });
                        let up_entity = paint_entity.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = up_entity.update(cx, |view, _| view.release_pointer(event));
                            }
                        });
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
