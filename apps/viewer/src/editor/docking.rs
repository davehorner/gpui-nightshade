use super::*;
mod persistence;
use gpui_kit::component::dock::{
    BasePanel, DockArea, DockLayout, DockPlacement, DockSkin, NodeId, PaneNode, PaneRef, Panel,
    PanelEvent, PanelId, panel_handle, register_panel,
};

#[derive(Clone, Copy)]
enum Kind {
    Hierarchy,
    Viewport,
    Inspector,
    Animation,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Hierarchy => "Scene",
            Self::Viewport => "Viewport",
            Self::Inspector => "Inspector",
            Self::Animation => "Animation",
        }
    }
}

struct EditorPanel {
    editor: WeakEntity<Editor>,
    kind: Kind,
    focus: FocusHandle,
    _observe: Subscription,
}

impl EventEmitter<PanelEvent> for EditorPanel {}
impl Focusable for EditorPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl BasePanel for EditorPanel {
    fn panel_name(&self) -> &'static str {
        self.kind.name()
    }

    fn closable(&self, _: &App) -> bool {
        false
    }
}
impl Panel for EditorPanel {
    fn inner_padding(&self, _: &App) -> bool {
        false
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.kind.name()
    }
}
impl Render for EditorPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.editor
            .update(cx, |editor, cx| match self.kind {
                Kind::Hierarchy => editor.hierarchy_panel(cx),
                Kind::Inspector => editor.inspector_panel(cx),
                Kind::Animation => {
                    let snapshot = editor.snapshot.clone();
                    div()
                        .size_full()
                        .children(snapshot.as_ref().map(|s| editor.animation_panel(s, cx)))
                        .into_any_element()
                }
                Kind::Viewport => div()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .child(editor.viewport.clone())
                    .when(editor.overlay, |pane| {
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
                    })
                    .into_any_element(),
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

/// Retains panel entities independently of their current split/tab placement.
/// Panels hold weak editor references; moving or resetting never rebuilds the worker.
pub(super) struct EditorDock {
    pub area: Entity<DockArea>,
    panels: [Entity<EditorPanel>; 4],
    path: Option<std::path::PathBuf>,
}

impl EditorDock {
    pub fn viewport_visible(&self, cx: &App) -> bool {
        let area = self.area.read(cx);
        let panel = PanelId::from(self.panels[1].entity_id());
        [
            DockPlacement::Center,
            DockPlacement::Left,
            DockPlacement::Right,
            DockPlacement::Bottom,
        ]
        .into_iter()
        .filter(|placement| *placement == DockPlacement::Center || area.is_dock_open(*placement))
        .filter_map(|placement| area.layout(placement))
        .any(|tree| displayed(tree.root(), panel, area.zoomed_group()))
    }
    pub fn new(editor: WeakEntity<Editor>, window: &mut Window, cx: &mut Context<Editor>) -> Self {
        let (area, _) = DockSkin::dock_area("nightshade-editor", Some(1), window, cx);
        let panels = [
            Kind::Hierarchy,
            Kind::Viewport,
            Kind::Inspector,
            Kind::Animation,
        ]
        .map(|kind| {
            cx.new(|cx| EditorPanel {
                editor: editor.clone(),
                kind,
                focus: cx.focus_handle(),
                _observe: cx.observe(&editor.upgrade().expect("editor is alive"), |_, _, cx| {
                    cx.notify()
                }),
            })
        });
        for panel in &panels {
            let name = panel.read(cx).kind.name();
            let panel = panel.downgrade();
            register_panel(cx, name, move |_, _, _| {
                panel_handle(panel.upgrade().expect("registered editor panel is alive"))
            });
        }
        let dock = Self {
            area,
            panels,
            path: persistence::layout_path(),
        };
        dock.reset(window, cx);
        if let Some(path) = &dock.path {
            match persistence::read(path) {
                Ok(Some(state)) => {
                    if let Err(error) = dock
                        .area
                        .update(cx, |area, cx| area.load(state, window, cx))
                    {
                        eprintln!("Could not restore editor layout: {error}");
                        dock.reset(window, cx);
                    }
                }
                Ok(None) => {}
                Err(error) => eprintln!("Ignored saved editor layout: {error}"),
            }
        }
        dock
    }

    pub fn save(&self, cx: &App) -> Result<bool, String> {
        if let Some(path) = &self.path {
            persistence::write(path, &self.area.read(cx).dump(cx))?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn reset(&self, window: &mut Window, cx: &mut Context<Editor>) {
        self.area.update(cx, |area, cx| {
            area.set_zoomed_out(window, cx);
            for placement in [
                DockPlacement::Left,
                DockPlacement::Right,
                DockPlacement::Bottom,
            ] {
                area.remove_dock(placement, window, cx);
            }
            let [scene, viewport, inspector, animation] = self.panels.clone();
            // Keep every panel in the center tree so resetting also removes splits
            // made by dragging. The viewport entity itself stays unchanged.
            area.set_center(
                DockLayout::v_split()
                    .child(
                        DockLayout::h_split()
                            .child(
                                DockLayout::tabs().panel_view(panel_handle(scene), cx),
                                Some(px(240.)),
                            )
                            .child(
                                DockLayout::tabs().panel_view(panel_handle(viewport), cx),
                                None,
                            )
                            .child(
                                DockLayout::tabs().panel_view(panel_handle(inspector), cx),
                                Some(px(310.)),
                            ),
                        None,
                    )
                    .child(
                        DockLayout::tabs().panel_view(panel_handle(animation), cx),
                        Some(px(145.)),
                    ),
                window,
                cx,
            );
        });
    }
}

fn displayed(node: &PaneNode, panel: PanelId, zoom: Option<NodeId>) -> bool {
    match node.kind() {
        PaneRef::Tabs { panels, active_ix } => {
            zoom.is_none_or(|id| id == node.id()) && panels.get(active_ix) == Some(&panel)
        }
        PaneRef::Split { children, .. } => {
            children.iter().any(|child| displayed(child, panel, zoom))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::component::dock::{PaneTree, PanelBuilder, PanelInfo, PanelState, RootKind};

    struct Builder;
    impl PanelBuilder for Builder {
        fn build(&mut self, state: &PanelState, _: &PanelInfo) -> PanelId {
            PanelId::from_u64(if state.panel_name == "Viewport" { 1 } else { 2 })
        }
    }
    #[test]
    fn visibility_follows_active_tab_and_zoomed_group() {
        let tabs = |names: &[&str], active| PanelState {
            panel_name: "TabPanel".into(),
            children: names.iter().map(|name| PanelState::new(*name)).collect(),
            info: PanelInfo::tabs(active),
        };
        let mut state = PanelState {
            panel_name: "StackPanel".into(),
            children: vec![tabs(&["Viewport", "Inspector"], 0), tabs(&["Scene"], 0)],
            info: PanelInfo::stack(vec![px(600.), px(240.)], Axis::Horizontal),
        };
        let tree = PaneTree::from_state(&state, RootKind::Split, &mut Builder);
        let panel = PanelId::from_u64(1);
        assert!(displayed(tree.root(), panel, None));
        let PaneRef::Split { children, .. } = tree.root().kind() else {
            panic!("split")
        };
        assert!(displayed(tree.root(), panel, Some(children[0].id())));
        assert!(!displayed(tree.root(), panel, Some(children[1].id())));
        state.children[0].info = PanelInfo::tabs(1);
        let tree = PaneTree::from_state(&state, RootKind::Split, &mut Builder);
        assert!(!displayed(tree.root(), panel, None));
    }
}
