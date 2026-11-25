use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use nightshade_viewer_core::{ALPHA_LABELS, MaterialControl};

const FIRST_FIELD: usize = 33;
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
        let mut panel = div().flex().flex_col().gap_2().child(alpha);
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
    use super::{CONTROLS, FIRST_FIELD, field_action};
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
