use crate::worker::{ViewRequest, ViewportFrame, Worker};
use gpui_kit::*;
use gpui_nightshade::presentation::{FrameInfo, PresentationStatus};
use std::{sync::Arc, time::Duration};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/touch.js")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = viewportTouchBounds)]
    fn viewport_touch_bounds(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        yaw: f32,
        pitch: f32,
        distance: f32,
        tx: f32,
        ty: f32,
        tz: f32,
    );
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = clearViewportTouch)]
    fn clear_viewport_touch();
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = takeViewportTouch)]
    fn take_viewport_touch() -> js_sys::Float32Array;
}

pub struct NightshadeViewport {
    worker: Worker,
    image: Option<ViewportFrame>,
    diagnostics: String,
    request: ViewRequest,
    camera_poses: [[f32; 6]; 4],
    last_pointer: Option<Point<Pixels>>,
    drag_button: Option<MouseButton>,
    scene_input: bool,
    visibility: crate::visibility::PresentationVisibility,
    input_subscriptions: Option<Vec<Subscription>>,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    scale: f32,
    pick_sequence: u64,
    poll_task: Option<Task<()>>,
}
impl NightshadeViewport {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::with_backend(gpui_nightshade::presentation::BackendPreference::Auto, cx)
    }
    pub fn with_backend(
        backend: gpui_nightshade::presentation::BackendPreference,
        cx: &mut Context<Self>,
    ) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let factory: crate::SceneFactory = Arc::new(gpui_nightshade::host::cube_viewport);
        #[cfg(target_arch = "wasm32")]
        let factory: crate::SceneFactory = Arc::new(|backends, width, height| {
            Box::pin(async move {
                Ok(Box::new(
                    gpui_nightshade::host::SceneHost::cube_with_backends(width, height, backends)
                        .await?,
                )
                    as Box<dyn gpui_nightshade::host::ViewportHost>)
            })
        });
        Self::from_factory(backend, factory, cx)
    }
    /// Creates a viewport whose scene is built entirely on the render worker.
    pub fn from_factory(
        backend: gpui_nightshade::presentation::BackendPreference,
        factory: crate::SceneFactory,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|view, cx| {
            #[cfg(target_arch = "wasm32")]
            clear_viewport_touch();
            if let Some(image) = view.image.take() {
                cx.drop_image(image.image, None);
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
        Self {
            worker: Worker::new(backend, factory),
            image: None,
            diagnostics: "Initializing".into(),
            request: ViewRequest {
                layout: Default::default(),
                active_view: 0,
                width: 640,
                height: 480,
                yaw: 0.6,
                pitch: 0.35,
                distance: 4.,
                target: [0.; 3],
                #[cfg(target_arch = "wasm32")]
                zoom_anchor: None,
                pick: None,
            },
            camera_poses: [[0.6, 0.35, 4., 0., 0., 0.]; 4],
            last_pointer: None,
            drag_button: None,
            scene_input: false,
            visibility: Default::default(),
            input_subscriptions: None,
            bounds: Bounds::default(),
            scale: 1.,
            pick_sequence: 0,
            focus: cx.focus_handle(),
            poll_task: None,
        }
    }
    /// Navigation pose shared by pointer navigation and embedding controls.
    pub fn camera_pose(&self) -> gpui_nightshade::host::CameraPose {
        gpui_nightshade::host::CameraPose {
            yaw: self.request.yaw,
            pitch: self.request.pitch,
            distance: self.request.distance,
            target: self.request.target,
        }
    }
    /// Place the active view without replacing its scene or presentation worker.
    pub fn set_camera_pose(
        &mut self,
        pose: gpui_nightshade::host::CameraPose,
        cx: &mut Context<Self>,
    ) {
        if ![
            pose.yaw,
            pose.pitch,
            pose.distance,
            pose.target[0],
            pose.target[1],
            pose.target[2],
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            return;
        }
        self.cancel_input();
        self.request.yaw = pose.yaw;
        #[cfg(target_arch = "wasm32")]
        {
            self.request.zoom_anchor = None;
        }
        self.request.pitch = pose.pitch.clamp(-1.5, 1.5);
        self.request.distance = pose.distance.clamp(0.01, 200.);
        self.request.target = pose.target;
        self.request.pick = None;
        self.worker.request(self.request);
        cx.notify();
    }
    pub fn camera_layout(&self) -> crate::CameraLayout {
        self.request.layout
    }
    pub fn set_camera_layout(&mut self, layout: crate::CameraLayout, cx: &mut Context<Self>) {
        self.cancel_input();
        self.request.layout = layout;
        self.focus_view(0);
        self.worker.request(self.request);
        cx.notify();
    }
    fn focus_view(&mut self, index: usize) {
        if index == self.request.active_view {
            return;
        }
        self.camera_poses[self.request.active_view] = [
            self.request.yaw,
            self.request.pitch,
            self.request.distance,
            self.request.target[0],
            self.request.target[1],
            self.request.target[2],
        ];
        self.request.active_view = index;
        let p = self.camera_poses[index];
        self.request.yaw = p[0];
        self.request.pitch = p[1];
        self.request.distance = p[2];
        self.request.target = [p[3], p[4], p[5]];
        self.request.pick = None;
    }
    fn focus_at(&mut self, position: Point<Pixels>) {
        if let Some(p) = self.local_position(position, false)
            && let Some(index) =
                self.request
                    .layout
                    .hit(self.request.width, self.request.height, p[0], p[1])
        {
            if index != self.request.active_view {
                self.cancel_input();
                self.focus_view(index);
            }
            self.worker.request(self.request);
        }
    }
    /// Owned snapshot of the actual transport, adapter, visibility and failures.
    pub fn presentation_status(&self) -> PresentationStatus {
        self.worker.presentation_status()
    }
    /// Metadata for the image currently retained by this view. This is not a
    /// swapchain-present acknowledgement or a GPU timing measurement.
    pub fn current_frame_info(&self) -> Option<FrameInfo> {
        self.image.as_ref().map(|frame| frame.info)
    }
    /// Use trunk scene input for left-button picking/gizmos; right-drag navigates.
    pub fn with_scene_input(mut self) -> Self {
        self.scene_input = true;
        self
    }
    /// Set container/panel visibility while retaining the scene worker and simulation.
    /// Presentation also requires the owning platform window to be visible.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visibility.set_panel(visible) {
            self.apply_visibility(cx);
        }
    }
    fn set_window_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visibility.set_window(visible) {
            self.apply_visibility(cx);
        }
    }
    fn apply_visibility(&mut self, cx: &mut Context<Self>) {
        self.cancel_input();
        #[cfg(target_arch = "wasm32")]
        if !self.visibility.is_presented() {
            clear_viewport_touch();
        }
        self.request.pick = None;
        self.worker.set_visible(self.visibility.is_presented());
        cx.notify();
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
        let (layout, active) = self.worker.camera_layout();
        if (self.request.layout, self.request.active_view) != (layout, active) {
            self.cancel_input();
            self.request.layout = layout;
            self.focus_view(active);
        }
        if self.poll_task.is_none() {
            self.poll_task = Some(cx.spawn_in(window, async |view, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                    if view
                        .update_in(cx, |view, window, cx| {
                            #[cfg(target_arch = "wasm32")]
                            if view.visibility.is_presented() {
                                let delta = take_viewport_touch().to_vec();
                                if delta.len() == 8 && delta[7] != 0. {
                                    view.cancel_input();
                                    view.focus_at(point(px(delta[5]), px(delta[6])));
                                    let local = view
                                        .local_position(point(px(delta[5]), px(delta[6])), true);
                                    view.request.zoom_anchor = local.and_then(|p| {
                                        let tile = view
                                            .request
                                            .layout
                                            .rects(view.request.width, view.request.height)
                                            .get(view.request.active_view)
                                            .copied()?;
                                        Some([
                                            (p[0] - tile.x as f32) / tile.width as f32,
                                            (p[1] - tile.y as f32) / tile.height as f32,
                                            tile.width as f32 / tile.height as f32,
                                        ])
                                    });
                                    view.request.target[0] -=
                                        delta[0] * view.request.distance * 0.002;
                                    view.request.target[1] +=
                                        delta[1] * view.request.distance * 0.002;
                                    view.request.yaw -= delta[2] * 0.01;
                                    view.request.pitch =
                                        (view.request.pitch + delta[3] * 0.01).clamp(-1.5, 1.5);
                                    view.request.distance = (view.request.distance
                                        * (-delta[4]).exp())
                                    .clamp(0.01, 200.);
                                    view.worker.request(view.request);
                                    cx.notify();
                                }
                            }
                            window.poll_external_images();
                            if view.visibility.is_presented() && view.worker.has_update() {
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        self.set_window_visible(window.is_visible(), cx);
        if self.input_subscriptions.is_none() {
            let focus = self.focus.clone();
            self.input_subscriptions = Some(vec![
                cx.observe_window_visibility(window, |view, visibility, _, cx| {
                    view.set_window_visible(visibility.is_visible(), cx);
                }),
                cx.on_focus_out(&focus, window, |view, _, _, _| view.cancel_input()),
                cx.observe_window_activation(window, |view, window, _| {
                    if !window.is_window_active() {
                        view.cancel_input();
                    }
                }),
            ]);
        }
        let (image, diagnostics) = self.worker.take();
        if let Some(image) = image {
            crate::benchmark::adopt(image.info, image.benchmark.as_ref());
            if let Some(old) = self.image.replace(image) {
                let _ = window.drop_image(old.image);
            }
        }
        self.diagnostics = diagnostics;
        let image = self.image.as_ref().map(|frame| frame.image.clone());
        let entity = cx.entity().downgrade();
        let paint_entity = entity.clone();
        div()
            .id("nightshade-viewport")
            .relative()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .on_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                let camera_button = MouseButton::Right;
                if event.button != MouseButton::Left && event.button != camera_button {
                    return;
                }
                let Some(point) = this.local_position(event.position, false) else {
                    return;
                };
                this.focus_at(event.position);
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
                #[cfg(target_arch = "wasm32")]
                {
                    this.request.zoom_anchor = None;
                }
                this.focus_at(event.position);
                let delta = event.delta.pixel_delta(px(16.)).y;
                this.request.distance =
                    (this.request.distance * (-f32::from(delta) * 0.003).exp()).clamp(0.01, 200.);
                this.worker.request(this.request);
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let scale = window.scale_factor();
                        let _ = entity.update(cx, |this, _| {
                            #[cfg(target_arch = "wasm32")]
                            viewport_touch_bounds(
                                bounds.origin.x.into(),
                                bounds.origin.y.into(),
                                bounds.size.width.into(),
                                bounds.size.height.into(),
                                this.request.yaw,
                                this.request.pitch,
                                this.request.distance,
                                this.request.target[0],
                                this.request.target[1],
                                this.request.target[2],
                            );
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
                            let count = this
                                .request
                                .layout
                                .rects(this.request.width, this.request.height)
                                .len();
                            this.focus_view(this.request.active_view.min(count - 1));
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
            .children(
                self.request
                    .layout
                    .rects(self.request.width, self.request.height)
                    .into_iter()
                    .enumerate()
                    .filter(|_| self.request.layout != crate::CameraLayout::Single)
                    .map(|(index, rect)| {
                        div()
                            .absolute()
                            .left(px(rect.x as f32 / self.scale))
                            .top(px(rect.y as f32 / self.scale))
                            .text_xs()
                            .px_2()
                            .bg(rgb(if index == self.request.active_view {
                                0x285f9e
                            } else {
                                0x202733
                            }))
                            .child(format!(
                                "Camera {}{}",
                                index + 1,
                                if index == self.request.active_view {
                                    " active"
                                } else {
                                    ""
                                }
                            ))
                    }),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_shrink_0()
                    .truncate()
                    .text_xs()
                    .p_1()
                    .child(self.diagnostics.clone()),
            )
    }
}
