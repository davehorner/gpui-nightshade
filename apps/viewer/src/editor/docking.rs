use super::*;
use gpui_kit::component::dock::{
    BasePanel, DockArea, DockLayout, DockPlacement, DockSkin, Panel, PanelEvent, panel_handle,
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
}

impl EditorDock {
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
        let dock = Self { area, panels };
        dock.reset(window, cx);
        dock
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
