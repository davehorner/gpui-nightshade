use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use nightshade_viewer_core::{
    ANTI_ALIASING_LABELS, ATMOSPHERE_LABELS, PBR_LABELS, SHADING_LABELS, TONEMAP_LABELS,
};
struct SettingMenu {
    id: &'static str,
    label: &'static str,
    labels: &'static [&'static str],
    current: usize,
    set: fn(usize) -> Action,
    disabled: Option<usize>,
}
impl Editor {
    fn setting_menu(&self, setting: SettingMenu, cx: &mut Context<Self>) -> AnyElement {
        let editor = cx.entity().downgrade();
        Button::new(setting.id)
            .label(format!(
                "{}: {}",
                setting.label,
                setting.labels.get(setting.current).unwrap_or(&"Unknown")
            ))
            .dropdown_menu(move |mut menu, _, _| {
                for (index, label) in setting.labels.iter().enumerate() {
                    let editor = editor.clone();
                    let set = setting.set;
                    menu = menu.item(
                        PopupMenuItem::new(*label)
                            .checked(index == setting.current)
                            .disabled(setting.disabled == Some(index))
                            .on_click(move |_, window, cx| {
                                let _ = editor.update(cx, |view, cx| {
                                    window.focus(&view.viewport.focus_handle(cx), cx);
                                    view.send(set(index), cx);
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }
    fn render_toggle(
        &self,
        id: &'static str,
        label: &'static str,
        enabled: bool,
        set: fn(bool) -> Action,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(format!("{label}: {}", if enabled { "on" } else { "off" }))
            .on_click(cx.listener(move |view, _, window, cx| {
                window.focus(&view.viewport.focus_handle(cx), cx);
                view.send(set(!enabled), cx);
            }))
    }
    pub(super) fn rendering_panel(
        &self,
        snapshot: &ViewerSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &snapshot.settings;
        let controls = &snapshot.render;
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .child("RENDERING — Enter to apply")
            .child("Exposure")
            .child(Input::new(&self.fields[6]))
            .child("Render scale (0.25–2)")
            .child(Input::new(&self.fields[24]));
        for setting in [
            SettingMenu {
                id: "shading-mode",
                label: "Shading",
                labels: &SHADING_LABELS,
                current: controls.shading,
                set: Action::SetShadingMode,
                disabled: None,
            },
            SettingMenu {
                id: "pbr-mode",
                label: "PBR debug",
                labels: &PBR_LABELS,
                current: controls.pbr_debug,
                set: Action::SetPbrDebug,
                disabled: None,
            },
            SettingMenu {
                id: "tonemap-mode",
                label: "Tonemap",
                labels: &TONEMAP_LABELS,
                current: controls.tonemap,
                set: Action::SetTonemap,
                disabled: None,
            },
            SettingMenu {
                id: "aa-mode",
                label: "AA",
                labels: &ANTI_ALIASING_LABELS,
                current: controls.anti_aliasing,
                set: Action::SetAntiAliasing,
                disabled: None,
            },
        ] {
            panel = panel.child(self.setting_menu(setting, cx));
        }
        panel = panel
            .child(self.render_toggle("grid", "Grid", snapshot.grid, Action::SetGrid, cx))
            .child(self.render_toggle(
                "normals",
                "Normals",
                controls.show_normals,
                Action::SetShowNormals,
                cx,
            ))
            .child(self.render_toggle(
                "bounds",
                "Bounds",
                controls.show_bounds,
                Action::SetShowBounds,
                cx,
            ))
            .child(self.render_toggle(
                "bloom",
                "Bloom",
                settings.bloom_enabled,
                Action::SetBloom,
                cx,
            ))
            .child(self.render_toggle("ssao", "SSAO", settings.ssao_enabled, Action::SetSsao, cx))
            .child(self.render_toggle("ssr", "SSR", settings.ssr_enabled, Action::SetSsr, cx))
            .child(self.render_toggle("ssgi", "SSGI", settings.ssgi_enabled, Action::SetSsgi, cx))
            .child("ENVIRONMENT")
            .child(self.setting_menu(
                SettingMenu {
                    id: "atmosphere-mode",
                    label: "Atmosphere",
                    labels: &ATMOSPHERE_LABELS,
                    current: controls.atmosphere,
                    set: Action::SetAtmosphere,
                    disabled: (!controls.hdr_loaded).then_some(ATMOSPHERE_LABELS.len() - 1),
                },
                cx,
            ))
            .child(self.render_toggle(
                "show-sky",
                "Sky",
                settings.show_sky,
                Action::SetShowSky,
                cx,
            ));
        if !controls.hdr_loaded {
            panel = panel.child("Load an HDR file or a sky from Assets to enable HDR atmosphere.");
        }
        for (index, label) in [
            (30, "Sun intensity"),
            (31, "Sun yaw (degrees)"),
            (32, "Sun pitch (degrees)"),
        ] {
            panel = panel.child(label).child(Input::new(&self.fields[index]));
        }
        panel = panel
            .child(self.render_toggle(
                "sun-shadows",
                "Sun shadows",
                controls.sun_shadows,
                Action::SetSunShadows,
                cx,
            ))
            .child("FOG")
            .child(self.render_toggle("fog", "Fog", settings.fog.is_some(), Action::SetFog, cx));
        for (index, label) in [(25, "Fog start"), (26, "Fog end"), (27, "Fog RGB")] {
            panel = panel.child(label).child(Input::new(&self.fields[index]));
        }
        panel = panel.child("DEPTH OF FIELD").child(self.render_toggle(
            "dof",
            "Depth of field",
            settings.depth_of_field.enabled,
            Action::SetDepthOfField,
            cx,
        ));
        for (index, label) in [(28, "Focus distance"), (29, "Focus range")] {
            panel = panel.child(label).child(Input::new(&self.fields[index]));
        }
        panel.into_any_element()
    }
}
