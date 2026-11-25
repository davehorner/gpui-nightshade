use gpui_kit::{
    component::{
        button::Button,
        input::{Input, InputEvent, InputState},
    },
    prelude::FluentBuilder,
};
use gpui_nightshade::presentation::BackendPreference;
use gpui_nightshade_components::{
    NightshadeViewport,
    gpui_kit::{self, *},
};
use nightshade_viewer_core::{
    GizmoMode, MaterialField, PrimitiveKind, ViewerAction as Action, ViewerArguments,
    ViewerConnection, ViewerSnapshot,
};
use std::{sync::Arc, time::Duration};

struct Editor {
    viewport: Entity<NightshadeViewport>,
    connection: ViewerConnection,
    snapshot: Option<Arc<ViewerSnapshot>>,
    fields: Vec<Entity<InputState>>,
    _subscriptions: Vec<Subscription>,
    _poll: Option<Task<()>>,
    message: String,
    overlay: bool,
}
impl Editor {
    fn new(
        viewport: Entity<NightshadeViewport>,
        connection: ViewerConnection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let labels = [
            "Name",
            "x, y, z",
            "x, y, z",
            "x, y, z",
            "Metallic",
            "Roughness",
            "Exposure",
            "Animation time",
        ];
        let fields: Vec<_> = labels
            .iter()
            .map(|label| cx.new(|cx| InputState::new(window, cx).placeholder(*label)))
            .collect();
        let subscriptions = fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                cx.subscribe_in(field, window, move |view, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        view.apply_field(index, cx);
                    }
                })
            })
            .collect();
        let poll = cx.spawn(async |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                if view
                    .update(cx, |view, cx| {
                        if let Some(snapshot) = view.connection.snapshot()
                            && view
                                .snapshot
                                .as_ref()
                                .is_none_or(|old| !Arc::ptr_eq(old, &snapshot))
                        {
                            view.snapshot = Some(snapshot);
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
            viewport,
            connection,
            snapshot: None,
            fields,
            _subscriptions: subscriptions,
            _poll: Some(poll),
            message: String::new(),
            overlay: false,
        }
    }
    fn send(&mut self, action: Action, cx: &mut Context<Self>) {
        self.message = if self.connection.submit(action).is_err() {
            "Action queue full; try again".into()
        } else {
            String::new()
        };
        cx.notify();
    }
    fn apply_field(&mut self, index: usize, cx: &mut Context<Self>) {
        let value = self.fields[index].read(cx).value().to_string();
        let scalar = || value.trim().parse::<f32>().ok().filter(|v| v.is_finite());
        let vector = || {
            let values: Option<Vec<f32>> = value
                .split([',', ' '])
                .filter(|v| !v.is_empty())
                .map(|v| v.parse::<f32>().ok().filter(|v| v.is_finite()))
                .collect();
            values.and_then(|v| v.try_into().ok())
        };
        let action = match index {
            0 => Some(Action::Rename(value.clone())),
            1 => vector().map(Action::SetTranslation),
            2 => vector().map(Action::SetRotation),
            3 => vector().map(Action::SetScale),
            4 => {
                scalar().map(|v| Action::SetMaterialField(MaterialField::Metallic(v.clamp(0., 1.))))
            }
            5 => scalar()
                .map(|v| Action::SetMaterialField(MaterialField::Roughness(v.clamp(0., 1.)))),
            6 => scalar().map(Action::SetExposure),
            7 => scalar().map(Action::SeekAnimation),
            _ => None,
        };
        if let Some(action) = action {
            self.send(action, cx);
        } else {
            self.message = "Enter finite numbers; vectors use x, y, z".into();
            cx.notify();
        }
    }
    fn sync_fields(&self, snapshot: &ViewerSnapshot, window: &mut Window, cx: &mut Context<Self>) {
        let vector = |v: [f32; 3]| format!("{:.3}, {:.3}, {:.3}", v[0], v[1], v[2]);
        let selection = &snapshot.selection;
        let values = [
            selection.name.clone(),
            vector(selection.translation),
            vector(selection.rotation),
            vector(selection.scale),
            format!("{:.3}", selection.material_values.metallic),
            format!("{:.3}", selection.material_values.roughness),
            format!("{:.3}", snapshot.settings.color_grading.exposure),
            format!("{:.3}", snapshot.animation.time),
        ];
        for (field, value) in self.fields.iter().zip(values) {
            if !field.focus_handle(cx).is_focused(window)
                && field.read(cx).value().as_ref() != value
            {
                field.update(cx, |field, cx| field.set_value(value, window, cx));
            }
        }
    }
    fn action_button(
        &self,
        id: &'static str,
        label: &'static str,
        action: Action,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .on_click(cx.listener(move |view, _, _, cx| view.send(action.clone(), cx)))
    }
}
impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.snapshot.clone();
        if let Some(snapshot) = &snapshot {
            self.sync_fields(snapshot, window, cx);
        }
        let mut tree = div()
            .id("scene-tree")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_y_scroll();
        if let Some(snapshot) = &snapshot {
            for node in &snapshot.nodes {
                let entity = node.entity;
                let selected = snapshot.selection.selected == Some(entity)
                    || snapshot.selection.others.contains(&entity);
                tree = tree.child(
                    div()
                        .id(("node", entity.id))
                        .px_2()
                        .py_1()
                        .ml(px(node.depth as f32 * 12.))
                        .bg(if selected {
                            rgb(0x33486b)
                        } else {
                            rgb(0x20242d)
                        })
                        .cursor_pointer()
                        .child(format!(
                            "{} {}",
                            if node.visible { "●" } else { "○" },
                            node.name
                        ))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                                window.focus(&view.viewport.focus_handle(cx), cx);
                                view.send(
                                    if event.modifiers.control {
                                        Action::ToggleInSelection(entity)
                                    } else {
                                        Action::Select(Some(entity))
                                    },
                                    cx,
                                );
                            }),
                        ),
                );
            }
        }
        let mut inspector = div()
            .id("inspector")
            .w(px(310.))
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(0x20242d));
        if let Some(snapshot) = &snapshot {
            if snapshot.selection.selected.is_some() {
                inspector = inspector.child("INSPECTOR — Enter to apply");
                for (index, label) in ["Name", "Translation", "Rotation (degrees)", "Scale"]
                    .iter()
                    .enumerate()
                {
                    inspector = inspector
                        .child(*label)
                        .child(Input::new(&self.fields[index]));
                }
                inspector = inspector.child(
                    div()
                        .flex()
                        .gap_1()
                        .child(self.action_button(
                            "visible",
                            "Visibility",
                            Action::ToggleSelectedVisible,
                            cx,
                        ))
                        .child(self.action_button("delete", "Delete", Action::DeleteSelected, cx)),
                );
                if snapshot.selection.material.is_some() {
                    inspector = inspector
                        .child("MATERIAL")
                        .child(snapshot.selection.material.clone().unwrap());
                    for (index, label) in [(4, "Metallic"), (5, "Roughness")] {
                        inspector = inspector
                            .child(label)
                            .child(Input::new(&self.fields[index]));
                    }
                    inspector = inspector.child(self.action_button(
                        "reset-material",
                        "Reset material",
                        Action::ResetMaterial,
                        cx,
                    ));
                }
                if let Some(light) = &snapshot.selection.light {
                    inspector = inspector.child(format!(
                        "Light intensity {:.2} · range {:.2}",
                        light.intensity, light.range
                    ));
                }
                if let Some(camera) = &snapshot.selection.camera {
                    inspector = inspector.child(format!(
                        "Camera FOV {:.2} · near {:.3} · far {:.1}",
                        camera.field_of_view, camera.near, camera.far
                    ));
                }
            } else {
                inspector = inspector.child("Select an entity in the hierarchy or viewport");
            }
            inspector = inspector
                .child("RENDERING")
                .child("Exposure")
                .child(Input::new(&self.fields[6]))
                .child(self.action_button(
                    "grid",
                    "Toggle grid",
                    Action::SetGrid(!snapshot.grid),
                    cx,
                ))
                .child(self.action_button(
                    "bloom",
                    "Toggle bloom",
                    Action::SetBloom(!snapshot.settings.bloom_enabled),
                    cx,
                ));
        }
        let mut timeline = div().flex().items_center().gap_2().p_2().bg(rgb(0x20242d));
        if let Some(snapshot) = &snapshot {
            timeline = timeline
                .child(self.action_button("play", "Play / pause", Action::TogglePlayback, cx))
                .child(self.action_button("stop", "Stop", Action::StopAnimation, cx))
                .child(Input::new(&self.fields[7]).w(px(100.)))
                .child(format!(
                    "{:.2} / {:.2}s · {} clips",
                    snapshot.animation.time,
                    snapshot.animation.duration,
                    snapshot.clips.len()
                ));
            for (index, clip) in snapshot.clips.iter().enumerate() {
                timeline = timeline.child(
                    Button::new(("clip", index))
                        .label(clip.name.clone())
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.send(Action::BlendToClip(index), cx)
                        })),
                );
            }
        }
        div()
            .key_context("NightshadeEditor")
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x171a21))
            .text_color(rgb(0xdce4f2))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if view
                    .fields
                    .iter()
                    .any(|field| field.focus_handle(cx).is_focused(window))
                {
                    return;
                }
                let key = event.keystroke.key.as_str();
                let modifiers = event.keystroke.modifiers;
                let action = match (key, modifiers.control || modifiers.platform) {
                    ("o", true) => Some(Action::ImportFile),
                    ("z", true) if modifiers.shift => Some(Action::Redo),
                    ("z", true) => Some(Action::Undo),
                    ("y", true) => Some(Action::Redo),
                    ("f", false) => Some(Action::FrameModel),
                    _ => None,
                };
                if let Some(action) = action {
                    view.send(action, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .p_2()
                    .items_center()
                    .child("NIGHTSHADE")
                    .child(self.action_button("open", "Open model", Action::ImportFile, cx))
                    .child(self.action_button(
                        "cube",
                        "Add cube",
                        Action::AddPrimitive(PrimitiveKind::Cube),
                        cx,
                    ))
                    .child(self.action_button("frame", "Frame", Action::FrameModel, cx))
                    .child(self.action_button("undo", "Undo", Action::Undo, cx))
                    .child(self.action_button("redo", "Redo", Action::Redo, cx))
                    .child(self.action_button(
                        "move",
                        "Move",
                        Action::SetGizmoMode(GizmoMode::Move),
                        cx,
                    ))
                    .child(self.action_button(
                        "rotate",
                        "Rotate",
                        Action::SetGizmoMode(GizmoMode::Rotate),
                        cx,
                    ))
                    .child(self.action_button(
                        "scale",
                        "Scale",
                        Action::SetGizmoMode(GizmoMode::Scale),
                        cx,
                    ))
                    .child(
                        Button::new("overlay")
                            .label("Overlay")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.overlay = !view.overlay;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(240.))
                            .h_full()
                            .bg(rgb(0x20242d))
                            .child(div().p_3().child("SCENE"))
                            .child(tree),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .child(self.viewport.clone())
                            .when(self.overlay, |pane| {
                                pane.child(
                                    div()
                                        .absolute()
                                        .top_4()
                                        .left_4()
                                        .p_4()
                                        .bg(rgb(0x35435c))
                                        .rounded_md()
                                        .child("GPUI overlay over the live viewer scene"),
                                )
                            }),
                    )
                    .child(inspector),
            )
            .child(timeline)
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .child(if self.message.is_empty() {
                        snapshot
                            .as_ref()
                            .map(|s| {
                                format!(
                                    "{} nodes · {} · {}",
                                    s.nodes.len(),
                                    if s.loading { "Loading" } else { "Ready" },
                                    s.notice.clone().unwrap_or_default()
                                )
                            })
                            .unwrap_or("Starting trunk viewer".into())
                    } else {
                        self.message.clone()
                    }),
            )
    }
}
pub fn launch(backend: BackendPreference, arguments: ViewerArguments) {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let connection = ViewerConnection::default();
                let worker_connection = connection.clone();
                let factory: gpui_nightshade_components::SceneFactory =
                    Arc::new(move |backends, width, height| {
                        nightshade_viewer_core::open(
                            arguments.clone(),
                            backends,
                            width,
                            height,
                            worker_connection.clone(),
                        )
                    });
                let viewport = cx.new(|cx| NightshadeViewport::from_factory(backend, factory, cx));
                cx.new(|cx| Editor::new(viewport, connection, window, cx))
            })
            .expect("open viewer");
            cx.activate(true);
        });
}
