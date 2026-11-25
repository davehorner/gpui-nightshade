mod animation;
mod docking;
mod rendering;
use animation::{WeightEditor, WeightTarget};
use gpui_kit::base::Disableable;
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
    BrowserItem, BrowserSnapshot, BrowserStatus, BrowserTab, CameraField, GizmoMode, LightField,
    LightKind, MaterialField, PrimitiveKind, TextureSlot, ViewerAction as Action, ViewerArguments,
    ViewerConnection, ViewerSnapshot,
};
use std::{collections::HashMap, sync::Arc, time::Duration};

struct Editor {
    dock: docking::EditorDock,
    viewport: Entity<NightshadeViewport>,
    connection: ViewerConnection,
    snapshot: Option<Arc<ViewerSnapshot>>,
    fields: Vec<Entity<InputState>>,
    _subscriptions: Vec<Subscription>,
    _poll: Option<Task<()>>,
    message: String,
    overlay: bool,
    field_selection: Option<(u32, u32)>,
    thumbnails: crate::thumbnails::Thumbnails,
    weights: HashMap<WeightTarget, WeightEditor>,
    render_page: bool,
    inspector_scroll: ScrollHandle,
}
impl Editor {
    fn new(
        viewport: Entity<NightshadeViewport>,
        connection: ViewerConnection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|view, cx| view.thumbnails.release(cx))
            .detach();
        let labels = [
            "Name",
            "x, y, z",
            "x, y, z",
            "x, y, z",
            "Metallic",
            "Roughness",
            "Exposure",
            "Animation time",
            "Base color RGBA",
            "Emissive RGB",
            "Emissive strength",
            "Transmission",
            "IOR",
            "Light RGB",
            "Intensity",
            "Range",
            "Inner cone degrees",
            "Outer cone degrees",
            "Field of view degrees",
            "Near",
            "Far (0 = infinite)",
            "Playback speed",
            "Search assets — Enter to apply",
            "Blend seconds",
            "Render scale (0.25–2)",
            "Fog start",
            "Fog end",
            "Fog RGB",
            "Focus distance",
            "Focus range",
            "Sun intensity",
            "Sun yaw degrees",
            "Sun pitch degrees",
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
                        if view.thumbnails.has_update() {
                            cx.notify();
                        }
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
        let dock = docking::EditorDock::new(cx.entity().downgrade(), window, cx);
        Self {
            dock,
            viewport,
            connection,
            snapshot: None,
            fields,
            _subscriptions: subscriptions,
            _poll: Some(poll),
            message: String::new(),
            overlay: false,
            field_selection: None,
            thumbnails: Default::default(),
            weights: HashMap::new(),
            render_page: false,
            inspector_scroll: ScrollHandle::new(),
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
        let scalar = || finite_scalar(&value);
        let vector = || finite_vector::<3>(&value);
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
            8 => finite_vector::<4>(&value).map(|v| {
                Action::SetMaterialField(MaterialField::BaseColor(v.map(|v| v.clamp(0., 1.))))
            }),
            9 => vector()
                .map(|v| Action::SetMaterialField(MaterialField::Emissive(v.map(|v| v.max(0.))))),
            10 => scalar()
                .filter(|v| *v >= 0.)
                .map(|v| Action::SetMaterialField(MaterialField::EmissiveStrength(v))),
            11 => scalar()
                .filter(|v| (0.0..=1.0).contains(v))
                .map(|v| Action::SetMaterialField(MaterialField::Transmission(v))),
            12 => scalar()
                .filter(|v| *v >= 1.)
                .map(|v| Action::SetMaterialField(MaterialField::Ior(v))),
            13 => vector()
                .filter(|v| v.iter().all(|v| *v >= 0.))
                .map(|v| Action::SetLightField(LightField::Color(v))),
            14 => scalar()
                .filter(|v| *v >= 0.)
                .map(|v| Action::SetLightField(LightField::Intensity(v))),
            15 => scalar()
                .filter(|v| *v >= 0.)
                .map(|v| Action::SetLightField(LightField::Range(v))),
            16 => scalar()
                .filter(|v| {
                    *v >= 0.
                        && *v
                            <= self
                                .snapshot
                                .as_ref()
                                .and_then(|s| s.selection.light.as_ref())
                                .map_or(90., |l| l.outer_cone)
                })
                .map(|v| Action::SetLightField(LightField::InnerCone(v))),
            17 => scalar()
                .filter(|v| {
                    *v <= 90.
                        && *v
                            >= self
                                .snapshot
                                .as_ref()
                                .and_then(|s| s.selection.light.as_ref())
                                .map_or(0., |l| l.inner_cone)
                })
                .map(|v| Action::SetLightField(LightField::OuterCone(v))),
            18 => scalar()
                .filter(|v| *v > 0. && *v < 180.)
                .map(|v| Action::SetCameraField(CameraField::FieldOfView(v))),
            19 => scalar()
                .filter(|v| {
                    *v > 0.
                        && self
                            .snapshot
                            .as_ref()
                            .and_then(|s| s.selection.camera.as_ref())
                            .is_some_and(|c| c.far == 0. || *v < c.far)
                })
                .map(|v| Action::SetCameraField(CameraField::Near(v))),
            20 => scalar()
                .filter(|v| {
                    self.snapshot
                        .as_ref()
                        .and_then(|s| s.selection.camera.as_ref())
                        .is_some_and(|c| (*v == 0. && c.perspective) || *v > c.near)
                })
                .map(|v| Action::SetCameraField(CameraField::Far(v))),
            21 => scalar().filter(|v| *v > 0.).map(Action::SetAnimationSpeed),
            22 => Some(Action::SetBrowserSearch(value.clone())),
            23 => scalar().filter(|v| *v >= 0.).map(Action::SetBlendTime),
            24 => scalar()
                .filter(|v| (0.25..=2.0).contains(v))
                .map(Action::SetRenderScale),
            25 => scalar().filter(|v| *v >= 0.).map(Action::SetFogStart),
            26 => scalar().filter(|v| *v > 0.).map(Action::SetFogEnd),
            27 => vector()
                .filter(|v| v.iter().all(|v| (0.0..=1.0).contains(v)))
                .map(Action::SetFogColor),
            28 => scalar()
                .filter(|v| *v >= 0.01)
                .map(Action::SetFocusDistance),
            29 => scalar().filter(|v| *v >= 0.01).map(Action::SetFocusRange),
            30 => scalar().filter(|v| *v >= 0.).map(Action::SetSunIntensity),
            31 => scalar().map(Action::SetSunYaw),
            32 => scalar().map(Action::SetSunPitch),
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
        let light = selection.light.as_ref();
        let camera = selection.camera.as_ref();
        let material = &selection.material_values;
        let values = [
            selection.name.clone(),
            vector(selection.translation),
            vector(selection.rotation),
            vector(selection.scale),
            format!("{:.3}", selection.material_values.metallic),
            format!("{:.3}", selection.material_values.roughness),
            format!("{:.3}", snapshot.settings.color_grading.exposure),
            format!("{:.3}", snapshot.animation.time),
            material.base_color.map(|v| format!("{v:.3}")).join(", "),
            vector(material.emissive),
            format!("{:.3}", material.emissive_strength),
            format!("{:.3}", material.transmission),
            format!("{:.3}", material.ior),
            vector(light.map_or([1.; 3], |v| v.color)),
            format!("{:.3}", light.map_or(0., |v| v.intensity)),
            format!("{:.3}", light.map_or(0., |v| v.range)),
            format!("{:.3}", light.map_or(0., |v| v.inner_cone)),
            format!("{:.3}", light.map_or(0., |v| v.outer_cone)),
            format!("{:.3}", camera.map_or(60., |v| v.field_of_view)),
            format!("{:.3}", camera.map_or(0.1, |v| v.near)),
            format!("{:.3}", camera.map_or(0., |v| v.far)),
            format!("{:.3}", snapshot.animation.speed),
            snapshot
                .browser
                .as_ref()
                .map_or(String::new(), |b| b.query.clone()),
            format!("{:.3}", snapshot.animation.blend_time),
            format!("{:.3}", snapshot.settings.render_scale),
            format!("{:.3}", snapshot.render.fog_start),
            format!("{:.3}", snapshot.render.fog_end),
            vector(snapshot.render.fog_color),
            format!("{:.3}", snapshot.settings.depth_of_field.focus_distance),
            format!("{:.3}", snapshot.settings.depth_of_field.focus_range),
            format!("{:.3}", snapshot.render.sun_intensity),
            format!("{:.3}", snapshot.render.sun_yaw),
            format!("{:.3}", snapshot.render.sun_pitch),
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
        let disabled = match &action {
            Action::Undo => self.snapshot.as_ref().is_none_or(|s| !s.can_undo),
            Action::Redo => self.snapshot.as_ref().is_none_or(|s| !s.can_redo),
            Action::TogglePlayback | Action::StopAnimation => {
                self.snapshot.as_ref().is_none_or(|s| s.clips.is_empty())
            }
            Action::ExportModel => self.snapshot.as_ref().is_none_or(|s| s.nodes.is_empty()),
            Action::ApplyHeldClips => self
                .snapshot
                .as_ref()
                .is_none_or(|s| !s.has_held_clips || s.joint_count == 0),
            _ => false,
        };
        Button::new(id)
            .disabled(disabled)
            .label(label)
            .on_click(cx.listener(move |view, _, window, cx| {
                window.focus(&view.viewport.focus_handle(cx), cx);
                view.send(action.clone(), cx);
            }))
    }
    fn browser_panel(&self, browser: &BrowserSnapshot, cx: &mut Context<Self>) -> AnyElement {
        let mut tabs = div().flex().items_center().gap_2().flex_wrap();
        for (index, (tab, label)) in [
            (BrowserTab::Library, "Local library"),
            (BrowserTab::Khronos, "Khronos"),
            (BrowserTab::Models, "Polyhaven models"),
            (BrowserTab::Skies, "Polyhaven skies"),
        ]
        .into_iter()
        .enumerate()
        {
            tabs = tabs.child(
                Button::new(("browser-tab", index))
                    .label(if browser.tab == tab {
                        format!("• {label}")
                    } else {
                        label.into()
                    })
                    .on_click(cx.listener(move |view, _, window, cx| {
                        window.focus(&view.viewport.focus_handle(cx), cx);
                        view.send(Action::SetBrowserTab(tab), cx);
                    })),
            );
        }
        tabs = tabs
            .child(Input::new(&self.fields[22]).w(px(220.)))
            .child(self.action_button("refresh-assets", "Refresh", Action::RefreshLibrary, cx));
        if browser.tab == BrowserTab::Library {
            tabs = tabs
                .child(self.action_button("add-folder", "Add folder", Action::AddLibraryFolder, cx))
                .child(self.action_button("add-files", "Add files", Action::AddLibraryFiles, cx));
        }
        if matches!(browser.tab, BrowserTab::Models | BrowserTab::Skies) {
            for (index, resolution) in [1, 2, 4, 8].into_iter().enumerate() {
                tabs = tabs.child(
                    Button::new(("asset-resolution", index))
                        .label(format!(
                            "{}{}K",
                            if browser.resolution == resolution {
                                "• "
                            } else {
                                ""
                            },
                            resolution
                        ))
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.send(Action::SetResolution(resolution), cx)
                        })),
                );
            }
        }
        tabs = tabs.child(self.action_button("close-assets", "Close", Action::CloseBrowser, cx));
        let mut entries = div()
            .id("asset-entries")
            .flex()
            .flex_wrap()
            .items_start()
            .gap_2()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        for (index, entry) in browser.entries.iter().enumerate() {
            let action = match &entry.item {
                BrowserItem::File(path) => Action::OpenLibraryFile(path.clone()),
                BrowserItem::Khronos(name) => Action::LoadKhronos(name.clone()),
                BrowserItem::Model(slug) => Action::LoadPolyhavenModel(slug.clone()),
                BrowserItem::Sky(slug) => Action::LoadPolyhavenSky(slug.clone()),
            };
            let mut card = div()
                .id(("asset-entry", index))
                .w(px(180.))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .bg(rgb(0x303948))
                .cursor_pointer();
            let mut preview = div()
                .w(px(128.))
                .h(px(96.))
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(0x202630));
            if let Some(image) = self.thumbnails.image(&entry.key) {
                preview = preview.child(
                    img(image)
                        .w(px(128.))
                        .h(px(96.))
                        .object_fit(ObjectFit::Contain),
                );
            } else {
                preview = preview.child(if !entry.key.starts_with("http") {
                    "Local file"
                } else if self.thumbnails.is_finished(&entry.key) {
                    "Preview unavailable"
                } else {
                    "Loading preview…"
                });
            }
            card = card.child(preview);
            entries = entries.child(card.child(entry.label.clone()).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _, window, cx| {
                    window.focus(&view.viewport.focus_handle(cx), cx);
                    view.send(action.clone(), cx);
                }),
            ));
        }
        if browser.entries.is_empty() {
            entries = entries.child(match browser.status {
                BrowserStatus::Loading => "Loading assets…",
                BrowserStatus::Failed => "Catalog request failed. Use Refresh to retry.",
                BrowserStatus::Idle => "Catalog has not started.",
                BrowserStatus::Ready => {
                    if browser.tab == BrowserTab::Library && browser.query.is_empty() {
                        "Add model files or a folder to the local library."
                    } else {
                        "No assets match this search."
                    }
                }
            });
        }
        let previous = browser.page.saturating_sub(1);
        let next = browser.page + 1;
        div()
            .h(px(260.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(rgb(0x252b35))
            .child(tabs)
            .child(entries)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("assets-prev")
                            .label("Previous")
                            .disabled(browser.page == 0)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.send(Action::SetBrowserPage(previous), cx)
                            })),
                    )
                    .child(format!(
                        "Page {} / {} · {:?}",
                        browser.page + 1,
                        browser.pages,
                        browser.status
                    ))
                    .child(
                        Button::new("assets-next")
                            .label("Next")
                            .disabled(next >= browser.pages)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.send(Action::SetBrowserPage(next), cx)
                            })),
                    ),
            )
            .into_any_element()
    }
}
impl Editor {
    fn hierarchy_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let snapshot = self.snapshot.clone();
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
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x20242d))
            .child(tree)
            .into_any_element()
    }
    fn inspector_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let snapshot = self.snapshot.clone();
        let mut inspector = div()
            .id("inspector-body")
            .track_scroll(&self.inspector_scroll)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(0x20242d));
        let inspector_tabs = div()
            .flex()
            .gap_2()
            .p_2()
            .child(
                Button::new("selection-tab")
                    .label("Selection")
                    .on_click(cx.listener(|view, _, window, cx| {
                        window.focus(&view.viewport.focus_handle(cx), cx);
                        view.render_page = false;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("render-tab")
                    .label("Rendering")
                    .on_click(cx.listener(|view, _, window, cx| {
                        window.focus(&view.viewport.focus_handle(cx), cx);
                        view.render_page = true;
                        cx.notify();
                    })),
            );
        if let Some(snapshot) = &snapshot {
            if self.render_page {
                inspector = inspector.child(self.rendering_panel(snapshot, cx));
            } else if snapshot.selection.selected.is_some() {
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
                if let Some(morphs) = self.morph_panel(snapshot, cx) {
                    inspector = inspector.child(morphs);
                }
                if snapshot.selection.material.is_some()
                    && snapshot.selection.light.is_none()
                    && snapshot.selection.camera.is_none()
                {
                    inspector = inspector
                        .child("MATERIAL")
                        .child(snapshot.selection.material.clone().unwrap());
                    for (index, label) in [
                        (8, "Base color RGBA"),
                        (4, "Metallic"),
                        (5, "Roughness"),
                        (9, "Emissive RGB"),
                        (10, "Emissive strength"),
                        (11, "Transmission"),
                        (12, "Index of refraction"),
                    ] {
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
                if snapshot.selection.material.is_some()
                    && snapshot.selection.light.is_none()
                    && snapshot.selection.camera.is_none()
                {
                    let values = &snapshot.selection.material_values;
                    inspector = inspector
                        .child(self.action_button(
                            "unlit",
                            "Toggle unlit",
                            Action::SetMaterialField(MaterialField::Unlit(!values.unlit)),
                            cx,
                        ))
                        .child(self.action_button(
                            "double-sided",
                            "Toggle double sided",
                            Action::SetMaterialField(MaterialField::DoubleSided(
                                !values.double_sided,
                            )),
                            cx,
                        ));
                    for slot in TextureSlot::ALL {
                        let texture = values
                            .textures
                            .iter()
                            .find(|(s, _)| *s == slot)
                            .map(|(_, name)| name.as_str())
                            .unwrap_or("None");
                        inspector = inspector
                            .child(format!("{}: {}", slot.label(), texture))
                            .child(
                                Button::new(("replace-texture", slot as usize))
                                    .label(format!("Replace {}", slot.label()))
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.send(Action::ReplaceTexture(slot), cx)
                                    })),
                            );
                        if texture != "None" {
                            inspector = inspector.child(
                                Button::new(("clear-texture", slot as usize))
                                    .label("Remove texture")
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.send(
                                            Action::SetMaterialField(MaterialField::Texture(
                                                slot, None,
                                            )),
                                            cx,
                                        )
                                    })),
                            );
                        }
                    }
                }
                if let Some(light) = &snapshot.selection.light {
                    inspector = inspector.child("LIGHT");
                    for (index, label) in [(13, "Color RGB"), (14, "Intensity")] {
                        inspector = inspector
                            .child(label)
                            .child(Input::new(&self.fields[index]));
                    }
                    if light.kind != 0 {
                        inspector = inspector.child("Range").child(Input::new(&self.fields[15]));
                    }
                    if light.kind == 2 {
                        for (index, label) in
                            [(16, "Inner cone (degrees)"), (17, "Outer cone (degrees)")]
                        {
                            inspector = inspector
                                .child(label)
                                .child(Input::new(&self.fields[index]));
                        }
                    }
                    inspector = inspector.child(self.action_button(
                        "shadows",
                        "Toggle shadows",
                        Action::SetLightField(LightField::CastShadows(!light.cast_shadows)),
                        cx,
                    ));
                }
                if let Some(camera) = &snapshot.selection.camera {
                    inspector = inspector
                        .child("CAMERA")
                        .child(self.action_button(
                            "use-camera",
                            "Use selected camera",
                            Action::SetActiveCamera(snapshot.selection.selected),
                            cx,
                        ))
                        .child(self.action_button(
                            "editor-camera",
                            "Use editor camera",
                            Action::SetActiveCamera(None),
                            cx,
                        ));
                    if camera.perspective {
                        inspector = inspector
                            .child("Vertical FOV (degrees)")
                            .child(Input::new(&self.fields[18]));
                    }
                    for (index, label) in [
                        (19, "Near clip"),
                        (
                            20,
                            if camera.perspective {
                                "Far clip (0 = infinite)"
                            } else {
                                "Far clip"
                            },
                        ),
                    ] {
                        inspector = inspector
                            .child(label)
                            .child(Input::new(&self.fields[index]));
                    }
                }
            } else {
                inspector = inspector.child("Select an entity in the hierarchy or viewport");
            }
        }
        let inspector = div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(0x20242d))
            .child(inspector_tabs)
            .child(inspector);
        inspector.into_any_element()
    }
}
impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.snapshot.clone();
        self.thumbnails.update(
            snapshot
                .as_ref()
                .and_then(|s| s.browser.as_ref())
                .into_iter()
                .flat_map(|b| b.entries.iter().map(|e| e.key.clone())),
            window,
        );
        if let Some(snapshot) = &snapshot {
            let selected = snapshot.selection.selected.map(|e| (e.id, e.generation));
            if selected != self.field_selection {
                if !self.render_page {
                    self.inspector_scroll.set_offset(point(px(0.), px(0.)));
                }
                if self.fields.iter().enumerate().any(|(i, field)| {
                    matches!(i,0..=5|8..=20) && field.focus_handle(cx).is_focused(window)
                }) {
                    window.focus(&self.viewport.focus_handle(cx), cx);
                }
                self.field_selection = selected;
            }
            self.sync_fields(snapshot, window, cx);
            self.sync_weight_fields(snapshot, window, cx);
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
                    || view
                        .weights
                        .values()
                        .any(|field| field.input.focus_handle(cx).is_focused(window))
                {
                    return;
                }
                let key = event.keystroke.key.as_str();
                let modifiers = event.keystroke.modifiers;
                let action = match (key, modifiers.control || modifiers.platform) {
                    ("o", true) => Some(Action::ImportFile),
                    ("b", true) => Some(Action::OpenBrowser(BrowserTab::Library)),
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
                    .flex_wrap()
                    .child("NIGHTSHADE")
                    .child(
                        Button::new("reset-layout")
                            .label("Reset layout")
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.dock.reset(window, cx);
                            })),
                    )
                    .child(self.action_button("open", "Open model", Action::ImportFile, cx))
                    .child(self.action_button(
                        "assets",
                        "Assets",
                        Action::OpenBrowser(BrowserTab::Library),
                        cx,
                    ))
                    .child(self.action_button(
                        "cube",
                        "Add cube",
                        Action::AddPrimitive(PrimitiveKind::Cube),
                        cx,
                    ))
                    .child(self.action_button(
                        "point-light",
                        "Add point light",
                        Action::AddLight(LightKind::Point),
                        cx,
                    ))
                    .child(self.action_button(
                        "spot-light",
                        "Add spot light",
                        Action::AddLight(LightKind::Spot),
                        cx,
                    ))
                    .child(self.action_button("frame", "Frame", Action::FrameModel, cx))
                    .child(self.action_button("screenshot", "Screenshot", Action::Screenshot, cx))
                    .child(self.action_button("export", "Export GLB", Action::ExportModel, cx))
                    .child(self.action_button("undo", "Undo", Action::Undo, cx))
                    .child(self.action_button("redo", "Redo", Action::Redo, cx))
                    .child(
                        self.action_button(
                            "move",
                            if snapshot
                                .as_ref()
                                .is_some_and(|s| s.gizmo.enabled && s.gizmo.mode == GizmoMode::Move)
                            {
                                "Move •"
                            } else {
                                "Move"
                            },
                            Action::SetGizmoMode(GizmoMode::Move),
                            cx,
                        ),
                    )
                    .child(
                        self.action_button(
                            "rotate",
                            if snapshot.as_ref().is_some_and(|s| {
                                s.gizmo.enabled && s.gizmo.mode == GizmoMode::Rotate
                            }) {
                                "Rotate •"
                            } else {
                                "Rotate"
                            },
                            Action::SetGizmoMode(GizmoMode::Rotate),
                            cx,
                        ),
                    )
                    .child(
                        self.action_button(
                            "scale",
                            if snapshot.as_ref().is_some_and(|s| {
                                s.gizmo.enabled && s.gizmo.mode == GizmoMode::Scale
                            }) {
                                "Scale •"
                            } else {
                                "Scale"
                            },
                            Action::SetGizmoMode(GizmoMode::Scale),
                            cx,
                        ),
                    )
                    .child(self.action_button(
                        "snap",
                        if snapshot.as_ref().is_some_and(|s| s.gizmo.snap) {
                            "Snap: on"
                        } else {
                            "Snap: off"
                        },
                        Action::SetSnap(!snapshot.as_ref().is_some_and(|s| s.gizmo.snap)),
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
            .when_some(
                snapshot.as_ref().and_then(|s| s.browser.as_ref()),
                |root, browser| root.child(self.browser_panel(browser, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.dock.area.clone()),
            )
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
            gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
            let bounds = Bounds::centered(None, size(px(1280.), px(720.)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                cx,
                |window, cx| {
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
                    let viewport = cx.new(|cx| {
                        NightshadeViewport::from_factory(backend, factory, cx).with_scene_input()
                    });
                    cx.new(|cx| Editor::new(viewport, connection, window, cx))
                },
            )
            .expect("open viewer");
            cx.activate(true);
        });
}

fn finite_scalar(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}
fn finite_vector<const N: usize>(value: &str) -> Option<[f32; N]> {
    value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|v| !v.is_empty())
        .map(finite_scalar)
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}
#[cfg(test)]
mod tests {
    use super::{finite_scalar, finite_vector};
    #[test]
    fn numeric_edits_reject_nonfinite_values_and_wrong_vector_dimensions() {
        assert_eq!(finite_vector::<3>("1, -2  3e2"), Some([1., -2., 300.]));
        assert_eq!(finite_vector::<4>("1, 0, 0, 1"), Some([1., 0., 0., 1.]));
        for text in ["NaN", "inf", "-inf", "1e99"] {
            assert_eq!(finite_scalar(text), None);
        }
        for text in ["1,2", "1,2,3,4", "1,NaN,3"] {
            assert_eq!(finite_vector::<3>(text), None);
        }
    }
}
