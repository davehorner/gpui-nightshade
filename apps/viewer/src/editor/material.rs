use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use nightshade_viewer_core::{ALPHA_LABELS, MaterialControl};
use std::time::Instant;

pub(super) struct OpacityEditor {
    pub slider: Entity<SliderState>,
    target: Option<(nightshade_viewer_core::SceneEntity, String)>,
    dragging: bool,
    blocked_until_release: bool,
    pending: Option<(f32, Instant)>,
}
impl OpacityEditor {
    pub fn new(cx: &mut Context<Editor>) -> Self {
        Self {
            slider: cx.new(|_| {
                SliderState::new()
                    .min(0.)
                    .max(1.)
                    .step(0.01)
                    .default_value(1.)
            }),
            target: None,
            dragging: false,
            blocked_until_release: false,
            pending: None,
        }
    }
}

fn opacity_action(snapshot: &ViewerSnapshot, opacity: f32) -> Option<Action> {
    snapshot.selection.material.as_ref()?;
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return None;
    }
    let mut color = snapshot.selection.material_values.base_color;
    color[3] = opacity;
    Some(Action::SetMaterialField(MaterialField::BaseColor(color)))
}

const FIRST_FIELD: usize = 33;

pub(super) fn field_error(index: usize, snapshot: Option<&ViewerSnapshot>) -> Option<String> {
    let (control, label) = CONTROLS.get(index.checked_sub(FIRST_FIELD)?)?;
    let Some(selection) = snapshot
        .map(|s| &s.selection)
        .filter(|s| s.material.is_some())
    else {
        return Some("Select an object with a material before editing.".into());
    };
    let (_, min, max, _) = control.row(&selection.material_values);
    Some(format!(
        "{label}: enter a finite number from {min} to {max}."
    ))
}
// Reuse trunk's bounds, values and action mapping rather than another material model.
pub(super) const CONTROLS: [(MaterialControl, &str); 11] = [
    (MaterialControl::AlphaCutoff, "Alpha cutoff"),
    (MaterialControl::NormalScale, "Normal scale"),
    (MaterialControl::OcclusionStrength, "Occlusion strength"),
    (MaterialControl::Specular, "Specular factor"),
    (MaterialControl::Thickness, "Volume thickness"),
    (MaterialControl::Dispersion, "Dispersion"),
    (MaterialControl::Clearcoat, "Clearcoat factor"),
    (MaterialControl::ClearcoatRoughness, "Clearcoat roughness"),
    (MaterialControl::SheenRoughness, "Sheen roughness"),
    (MaterialControl::Iridescence, "Iridescence factor"),
    (MaterialControl::IridescenceIor, "Iridescence IOR"),
];

pub(super) fn field_action(
    index: usize,
    text: &str,
    snapshot: Option<&ViewerSnapshot>,
) -> Option<Action> {
    let (control, _) = CONTROLS.get(index.checked_sub(FIRST_FIELD)?)?;
    let selection = &snapshot?.selection;
    selection.material.as_ref()?;
    let (_, min, max, _) = control.row(&selection.material_values);
    let value = finite_scalar(text).filter(|v| (min..=max).contains(v))?;
    Some(Action::SetMaterialField(control.field(value)))
}

impl Editor {
    pub(super) fn apply_opacity(&mut self, event: &SliderEvent, cx: &mut Context<Self>) {
        let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
        if self.opacity.blocked_until_release {
            if matches!(event, SliderEvent::Release(_)) {
                self.opacity.blocked_until_release = false;
            }
            return;
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let target = snapshot
            .selection
            .selected
            .zip(snapshot.selection.material.clone());
        if target.is_none() || target != self.opacity.target {
            return;
        }
        let Some(action) = opacity_action(snapshot, value.start()) else {
            return;
        };
        self.opacity.dragging = matches!(event, SliderEvent::Change(_));
        self.opacity.pending = Some((value.start(), Instant::now()));
        self.send(action, cx);
    }

    pub(super) fn sync_opacity(
        &mut self,
        snapshot: &ViewerSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = snapshot
            .selection
            .selected
            .zip(snapshot.selection.material.clone());
        if target != self.opacity.target {
            self.opacity.blocked_until_release |= self.opacity.dragging;
            self.opacity.target = target;
            self.opacity.dragging = false;
            self.opacity.pending = None;
        }
        let value = snapshot.selection.material_values.base_color[3].clamp(0., 1.);
        if let Some((pending, since)) = self.opacity.pending {
            if (pending - value).abs() < 0.0001 || since.elapsed() > Duration::from_secs(1) {
                self.opacity.pending = None;
            } else {
                return;
            }
        }
        if !self.opacity.dragging
            && (self.opacity.slider.read(cx).value().start() - value).abs() > 0.0001
        {
            self.opacity
                .slider
                .update(cx, |slider, cx| slider.set_value(value, window, cx));
        }
    }

    pub(super) fn advanced_material_panel(
        &self,
        snapshot: &ViewerSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = snapshot.selection.material_values.alpha_mode;
        let editor = cx.entity().downgrade();
        let alpha = Button::new("material-alpha-mode")
            .label(format!(
                "Alpha mode: {}",
                ALPHA_LABELS.get(current).unwrap_or(&"Unknown")
            ))
            .dropdown_menu(move |mut menu, _, _| {
                for (index, label) in ALPHA_LABELS.iter().enumerate() {
                    let editor = editor.clone();
                    menu = menu.item(
                        PopupMenuItem::new(*label)
                            .checked(index == current)
                            .on_click(move |_, _, cx| {
                                let _ = editor.update(cx, |view, cx| {
                                    view.send(
                                        Action::SetMaterialField(MaterialField::AlphaMode(index)),
                                        cx,
                                    );
                                });
                            }),
                    );
                }
                menu
            });
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(alpha)
            .child(format!(
                "Opacity: {:.0}%",
                self.opacity.slider.read(cx).value().start() * 100.
            ))
            .child(Slider::new(&self.opacity.slider).accessibility_label("Material opacity"))
            .child(if current == 0 {
                "Choose Blend mode to display partial opacity."
            } else {
                "Opacity edits base-color alpha; RGB stays unchanged."
            });
        for (offset, (control, label)) in CONTROLS.iter().enumerate() {
            let (_, min, max, _) = control.row(&snapshot.selection.material_values);
            panel = panel
                .child(format!("{label} ({min}–{max})"))
                .child(Input::new(&self.fields[FIRST_FIELD + offset]));
            if offset == 0 {
                panel = panel.child("Cutoff controls the Mask alpha threshold.");
            }
        }
        panel.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{CONTROLS, FIRST_FIELD, field_action, field_error, opacity_action};
    use nightshade_viewer_core::{
        ALPHA_LABELS, MaterialField, PrimitiveKind, ViewerAction as Action, ViewerArguments,
        ViewerCorePlugin, compose, snapshot, submit,
    };

    #[test]
    fn advanced_material_edits_reach_effective_snapshots_and_history() {
        let mut app = compose(
            ViewerArguments {
                no_agent: true,
                preview: true,
                ..Default::default()
            },
            ViewerCorePlugin {
                persist_settings: false,
                remote_catalogs: false,
            },
        );
        app.stages.apply_orderings();
        app.startup.run(&mut app.world);
        submit(&mut app.world, Action::AddPrimitive(PrimitiveKind::Cube)).unwrap();
        for _ in 0..3 {
            app.world.increment_tick();
            app.stages.run(&mut app.world);
        }
        let initial = snapshot(&app.world);
        assert!(initial.selection.material.is_some());
        let action = opacity_action(&initial, 0.3).unwrap();
        submit(&mut app.world, action).unwrap();
        app.world.increment_tick();
        app.stages.run(&mut app.world);
        let changed = snapshot(&app.world).selection.material_values.base_color;
        assert_eq!(
            &changed[..3],
            &initial.selection.material_values.base_color[..3]
        );
        assert_eq!(changed[3], 0.3);
        for invalid in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            assert!(opacity_action(&initial, invalid).is_none());
        }
        submit(&mut app.world, Action::Undo).unwrap();
        app.world.increment_tick();
        app.stages.run(&mut app.world);
        assert_eq!(
            snapshot(&app.world).selection.material_values.base_color,
            initial.selection.material_values.base_color
        );
        for mode in 0..ALPHA_LABELS.len() {
            submit(
                &mut app.world,
                Action::SetMaterialField(MaterialField::AlphaMode(mode)),
            )
            .unwrap();
            app.world.increment_tick();
            app.stages.run(&mut app.world);
            assert_eq!(
                snapshot(&app.world).selection.material_values.alpha_mode,
                mode
            );
        }
        submit(&mut app.world, Action::Undo).unwrap();
        app.world.increment_tick();
        app.stages.run(&mut app.world);
        assert_eq!(
            snapshot(&app.world).selection.material_values.alpha_mode,
            initial.selection.material_values.alpha_mode
        );
        submit(&mut app.world, Action::Redo).unwrap();
        app.world.increment_tick();
        app.stages.run(&mut app.world);
        assert_eq!(snapshot(&app.world).selection.material_values.alpha_mode, 3);

        for (offset, (control, _)) in CONTROLS.iter().enumerate() {
            let state = snapshot(&app.world);
            let (_, min, max, _) = control.row(&state.selection.material_values);
            let value = min + (max - min) * 0.37;
            let index = FIRST_FIELD + offset;
            let message = field_error(index, Some(&state)).unwrap();
            assert!(message.contains(CONTROLS[offset].1));
            assert!(message.contains(&format!("from {min} to {max}")));
            for bad in [
                "NaN".to_owned(),
                "inf".into(),
                (min - 1.).to_string(),
                (max + 1.).to_string(),
            ] {
                assert!(field_action(index, &bad, Some(&state)).is_none());
            }
            assert!(field_action(index, "0.5", None).is_none());
            let action = field_action(index, &value.to_string(), Some(&state)).unwrap();
            submit(&mut app.world, action).unwrap();
            app.world.increment_tick();
            app.stages.run(&mut app.world);
            let effective = snapshot(&app.world);
            assert!((control.row(&effective.selection.material_values).3 - value).abs() < 1e-5);
        }
        submit(&mut app.world, Action::ResetMaterial).unwrap();
        app.world.increment_tick();
        app.stages.run(&mut app.world);
        assert_eq!(
            snapshot(&app.world).selection.material_values,
            initial.selection.material_values
        );
    }
}
