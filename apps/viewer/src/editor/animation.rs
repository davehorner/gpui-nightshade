use super::*;
use nightshade_viewer_core::SceneEntity;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum WeightTarget {
    Morph {
        entity: SceneEntity,
        index: usize,
    },
    Layer {
        root: SceneEntity,
        index: usize,
        clip: usize,
    },
}
impl WeightTarget {
    fn action(self, snapshot: &ViewerSnapshot, weight: f32) -> Option<Action> {
        if !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
            return None;
        }
        match self {
            Self::Morph { entity, index }
                if snapshot.selection.selected == Some(entity)
                    && snapshot
                        .morphs
                        .iter()
                        .any(|m| m.entity == entity && index < m.weights.len()) =>
            {
                Some(Action::SetMorphWeight {
                    entity,
                    index,
                    weight,
                })
            }
            Self::Layer { root, index, clip }
                if snapshot.animation_root == Some(root)
                    && snapshot
                        .animation
                        .layers
                        .get(index)
                        .is_some_and(|layer| layer.clip == clip) =>
            {
                Some(Action::SetLayerWeight { index, weight })
            }
            _ => None,
        }
    }
}
pub(super) struct WeightEditor {
    pub(super) input: Entity<InputState>,
    _subscription: Subscription,
}
impl Editor {
    pub(super) fn sync_weight_fields(
        &mut self,
        snapshot: &ViewerSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut wanted = HashMap::new();
        for morph in snapshot
            .morphs
            .iter()
            .filter(|m| snapshot.selection.selected == Some(m.entity))
        {
            for (index, weight) in morph.weights.iter().copied().enumerate() {
                wanted.insert(
                    WeightTarget::Morph {
                        entity: morph.entity,
                        index,
                    },
                    weight,
                );
            }
        }
        if let Some(root) = snapshot.animation_root {
            for (index, layer) in snapshot.animation.layers.iter().enumerate() {
                wanted.insert(
                    WeightTarget::Layer {
                        root,
                        index,
                        clip: layer.clip,
                    },
                    layer.weight,
                );
            }
        }
        if self.weights.iter().any(|(target, field)| {
            !wanted.contains_key(target) && field.input.focus_handle(cx).is_focused(window)
        }) {
            window.focus(&self.viewport.focus_handle(cx), cx);
        }
        self.weights.retain(|target, _| wanted.contains_key(target));
        for (target, value) in wanted {
            let text = format!("{value:.3}");
            if let Some(field) = self.weights.get(&target) {
                if !field.input.focus_handle(cx).is_focused(window)
                    && field.input.read(cx).value().as_ref() != text
                {
                    field
                        .input
                        .update(cx, |field, cx| field.set_value(text, window, cx));
                }
                continue;
            }
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("0–1")
                    .default_value(text)
            });
            let subscription = cx.subscribe_in(
                &input,
                window,
                move |view, input, event: &InputEvent, _, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. }) {
                        return;
                    }
                    let action = finite_scalar(&input.read(cx).value()).and_then(|weight| {
                        view.snapshot
                            .as_deref()
                            .and_then(|snapshot| target.action(snapshot, weight))
                    });
                    if let Some(action) = action {
                        view.send(action, cx);
                    } else {
                        view.message = "Enter a weight from 0 to 1 for the current scene".into();
                        cx.notify();
                    }
                },
            );
            self.weights.insert(
                target,
                WeightEditor {
                    input,
                    _subscription: subscription,
                },
            );
        }
    }
    pub(super) fn morph_panel(
        &self,
        snapshot: &ViewerSnapshot,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mut panel = div().flex().flex_col().gap_2();
        let mut count = 0;
        for morph in snapshot
            .morphs
            .iter()
            .filter(|m| snapshot.selection.selected == Some(m.entity))
        {
            panel = panel.child(format!("MORPH TARGETS · {}", morph.name));
            if snapshot.animation.playing {
                panel = panel.child("Pause playback to edit animation-driven weights.");
            }
            for index in 0..morph.weights.len() {
                if let Some(field) = self.weights.get(&WeightTarget::Morph {
                    entity: morph.entity,
                    index,
                }) {
                    panel = panel
                        .child(format!("Target {index} (0–1)"))
                        .child(Input::new(&field.input));
                    count += 1;
                }
            }
        }
        (count > 0).then(|| panel.into_any_element())
    }
    pub(super) fn animation_panel(
        &self,
        snapshot: &ViewerSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let controls = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(self.action_button(
                "play",
                if snapshot.animation.playing {
                    "Pause"
                } else {
                    "Play"
                },
                Action::TogglePlayback,
                cx,
            ))
            .child(self.action_button("stop", "Stop", Action::StopAnimation, cx))
            .child("Time")
            .child(Input::new(&self.fields[7]).w(px(90.)))
            .child(format!("/ {:.2}s", snapshot.animation.duration))
            .child("Speed")
            .child(Input::new(&self.fields[21]).w(px(70.)))
            .child("Blend (s)")
            .child(Input::new(&self.fields[23]).w(px(70.)))
            .child(self.action_button(
                "loop",
                if snapshot.animation.looping {
                    "Loop: on"
                } else {
                    "Loop: off"
                },
                Action::SetAnimationLoop(!snapshot.animation.looping),
                cx,
            ))
            .child(self.action_button(
                "skeleton",
                if snapshot.skeleton_enabled {
                    "Skeleton: on"
                } else {
                    "Skeleton: off"
                },
                Action::SetShowSkeleton(!snapshot.skeleton_enabled),
                cx,
            ))
            .child(format!("{} joints", snapshot.joint_count))
            .child(self.action_button(
                "apply-clips",
                "Apply held clips",
                Action::ApplyHeldClips,
                cx,
            ));
        let mut clips = div().flex().flex_wrap().items_center().gap_2();
        if snapshot.clips.is_empty() {
            clips = clips.child("No animation clips in this model");
        }
        for (index, clip) in snapshot.clips.iter().enumerate() {
            clips = clips
                .child(
                    Button::new(("play-clip", index))
                        .label(format!("Play {}", clip.name))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            window.focus(&view.viewport.focus_handle(cx), cx);
                            view.send(Action::BlendToClip(index), cx);
                        })),
                )
                .child(
                    Button::new(("add-layer", index))
                        .label(format!("Layer + {}", clip.name))
                        .disabled(snapshot.animation.layers.iter().any(|l| l.clip == index))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            window.focus(&view.viewport.focus_handle(cx), cx);
                            view.send(
                                Action::AddAnimationLayer {
                                    clip: index,
                                    weight: 1.,
                                },
                                cx,
                            );
                        })),
                );
        }
        let mut layers = div().flex().flex_wrap().items_center().gap_2();
        if let Some(root) = snapshot.animation_root {
            for (index, layer) in snapshot.animation.layers.iter().enumerate() {
                if let Some(field) = self.weights.get(&WeightTarget::Layer {
                    root,
                    index,
                    clip: layer.clip,
                }) {
                    layers = layers.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(format!("Layer: {}", layer.name))
                            .child(Input::new(&field.input).w(px(70.)))
                            .child(
                                Button::new(("remove-layer", index))
                                    .label("Remove")
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        window.focus(&view.viewport.focus_handle(cx), cx);
                                        view.send(Action::RemoveAnimationLayer(index), cx);
                                    })),
                            ),
                    );
                }
            }
        }
        div()
            .id("animation-panel")
            .max_h(px(180.))
            .overflow_y_scroll()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(rgb(0x20242d))
            .child(controls)
            .child(clips)
            .child(layers)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::WeightTarget;
    use nightshade_viewer_core::{
        LayerView, MorphInfo, SceneEntity, ViewerAction, ViewerArguments, ViewerCorePlugin,
    };
    #[test]
    fn weight_edits_reject_stale_scene_bindings_and_invalid_weights() {
        let app = nightshade_viewer_core::compose(
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
        let mut snapshot = nightshade_viewer_core::snapshot(&app.world);
        let entity = SceneEntity {
            id: 3,
            generation: 2,
        };
        snapshot.selection.selected = Some(entity);
        snapshot.morphs = vec![MorphInfo {
            entity,
            name: "Morph".into(),
            weights: vec![0.],
        }];
        let morph = WeightTarget::Morph { entity, index: 0 };
        assert!(
            matches!(morph.action(&snapshot,0.7),Some(ViewerAction::SetMorphWeight {weight,..}) if weight==0.7)
        );
        for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(morph.action(&snapshot, value).is_none());
        }
        assert!(
            WeightTarget::Morph { entity, index: 1 }
                .action(&snapshot, 0.5)
                .is_none()
        );
        snapshot.selection.selected = None;
        assert!(morph.action(&snapshot, 0.5).is_none());
        snapshot.animation_root = Some(entity);
        snapshot.animation.layers = vec![LayerView {
            clip: 4,
            name: "Layer".into(),
            weight: 1.,
        }];
        let layer = WeightTarget::Layer {
            root: entity,
            index: 0,
            clip: 4,
        };
        assert!(layer.action(&snapshot, 0.5).is_some());
        snapshot.animation_root = Some(SceneEntity {
            generation: 3,
            ..entity
        });
        assert!(
            layer.action(&snapshot, 0.5).is_none(),
            "a replaced model cannot receive a stale weight edit"
        );
        snapshot.animation_root = Some(entity);
        snapshot.animation.layers[0].clip = 5;
        assert!(
            layer.action(&snapshot, 0.5).is_none(),
            "a removed/replaced layer cannot receive a stale edit"
        );
    }
}
