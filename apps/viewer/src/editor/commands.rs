//! A GPUI command palette backed by the existing viewer dispatcher.
use super::*;
use gpui_kit::component::{
    WindowExt,
    command::{Command, CommandItem, CommandState},
};
use std::{cell::Cell, rc::Rc};

actions!(nightshade_editor, [OpenCommandPalette]);

#[derive(Clone)]
enum EditorCommand {
    Viewer(Action),
    SaveLayout,
    ResetLayout,
}
struct Entry {
    label: &'static str,
    command: EditorCommand,
    enabled: bool,
}
#[derive(Default)]
struct Availability {
    ready: bool,
    loaded: bool,
    selected: bool,
    undo: bool,
    redo: bool,
    snap: bool,
    grid: bool,
    clips: bool,
    joints: bool,
    skeleton: bool,
}
impl Availability {
    fn from_snapshot(snapshot: Option<&ViewerSnapshot>) -> Self {
        let Some(s) = snapshot else {
            return Self::default();
        };
        Self {
            ready: true,
            loaded: !s.nodes.is_empty(),
            selected: s.selection.selected.is_some(),
            undo: s.can_undo,
            redo: s.can_redo,
            snap: s.gizmo.snap,
            grid: s.grid,
            clips: !s.clips.is_empty(),
            joints: s.joint_count > 0,
            skeleton: s.skeleton_enabled,
        }
    }
}
fn entries(s: &Availability) -> Vec<Entry> {
    use EditorCommand::*;
    let mut result = vec![];
    let mut viewer = |label, action, enabled| {
        result.push(Entry {
            label,
            command: Viewer(action),
            enabled,
        })
    };
    viewer("Open model…", Action::ImportFile, true);
    viewer(
        "Browse local assets",
        Action::OpenBrowser(BrowserTab::Library),
        true,
    );
    viewer(
        "Browse Khronos models",
        Action::OpenBrowser(BrowserTab::Khronos),
        true,
    );
    viewer(
        "Browse Polyhaven models",
        Action::OpenBrowser(BrowserTab::Models),
        true,
    );
    viewer(
        "Browse Polyhaven skies",
        Action::OpenBrowser(BrowserTab::Skies),
        true,
    );
    viewer("Add library folder…", Action::AddLibraryFolder, true);
    viewer("Add library files…", Action::AddLibraryFiles, true);
    viewer("Refresh local library", Action::RefreshLibrary, true);
    for (label, kind) in [
        ("Add cube", PrimitiveKind::Cube),
        ("Add sphere", PrimitiveKind::Sphere),
        ("Add cylinder", PrimitiveKind::Cylinder),
        ("Add cone", PrimitiveKind::Cone),
        ("Add torus", PrimitiveKind::Torus),
        ("Add plane", PrimitiveKind::Plane),
    ] {
        viewer(label, Action::AddPrimitive(kind), true);
    }
    for (label, kind) in [
        ("Add point light", LightKind::Point),
        ("Add spot light", LightKind::Spot),
        ("Add directional light", LightKind::Directional),
    ] {
        viewer(label, Action::AddLight(kind), true);
    }
    viewer("Frame scene", Action::FrameModel, s.loaded);
    viewer("Use editor camera", Action::SetActiveCamera(None), true);
    viewer(
        "Toggle selected visibility",
        Action::ToggleSelectedVisible,
        s.selected,
    );
    viewer("Isolate selection", Action::ToggleIsolate, s.selected);
    viewer("Delete selection", Action::DeleteSelected, s.selected);
    viewer("Move gizmo", Action::SetGizmoMode(GizmoMode::Move), true);
    viewer(
        "Rotate gizmo",
        Action::SetGizmoMode(GizmoMode::Rotate),
        true,
    );
    viewer("Scale gizmo", Action::SetGizmoMode(GizmoMode::Scale), true);
    viewer("Toggle snapping", Action::SetSnap(!s.snap), s.ready);
    viewer("Toggle grid", Action::SetGrid(!s.grid), s.ready);
    viewer("Play / pause animation", Action::TogglePlayback, s.clips);
    viewer("Stop animation", Action::StopAnimation, s.clips);
    viewer(
        "Toggle skeleton",
        Action::SetShowSkeleton(!s.skeleton),
        s.joints,
    );
    viewer("Save screenshot…", Action::Screenshot, s.loaded);
    viewer("Export GLB…", Action::ExportModel, s.loaded);
    viewer("Undo", Action::Undo, s.undo);
    viewer("Redo", Action::Redo, s.redo);
    result.push(Entry {
        label: "Save layout",
        command: SaveLayout,
        enabled: true,
    });
    result.push(Entry {
        label: "Reset layout",
        command: ResetLayout,
        enabled: true,
    });
    result
}
// Resolve against current state at confirmation, not the state when opened.
fn resolve(index: usize, state: &Availability) -> Option<EditorCommand> {
    entries(state)
        .into_iter()
        .nth(index)
        .filter(|entry| entry.enabled)
        .map(|entry| entry.command)
}

impl Editor {
    pub(super) fn open_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let state = cx.new(|cx| CommandState::new(window, cx));
        let owner = cx.weak_entity();
        let focus_on_mount = Rc::new(Cell::new(true));
        window.open_dialog(cx, move |dialog, _, _| {
            let state = state.clone();
            let owner = owner.clone();
            let focus = focus_on_mount.clone();
            dialog
                .title("Commands")
                .width(px(560.))
                .content(move |content, window, cx| {
                    if focus.replace(false) {
                        let state = state.clone();
                        window.defer(cx, move |window, cx| {
                            state.read(cx).focus_handle(cx).focus(window, cx)
                        });
                    }
                    let availability = owner
                        .read_with(cx, |view, _| {
                            Availability::from_snapshot(view.snapshot.as_deref())
                        })
                        .unwrap_or_default();
                    let confirm_owner = owner.clone();
                    content.child(
                        Command::new(&state)
                            .placeholder("Search commands…")
                            .highlight_matches(true)
                            .max_h(px(360.))
                            .items(entries(&availability).into_iter().map(|entry| {
                                CommandItem::new()
                                    .label(entry.label)
                                    .disabled(!entry.enabled)
                            }))
                            .on_confirm(move |index, window, cx| {
                                let _ = confirm_owner.update(cx, |view, cx| {
                                    let current =
                                        Availability::from_snapshot(view.snapshot.as_deref());
                                    if let Some(command) = resolve(index.row, &current) {
                                        window.close_dialog(cx);
                                        match command {
                                            EditorCommand::Viewer(action) => view.send(action, cx),
                                            EditorCommand::SaveLayout => view.save_layout(cx),
                                            EditorCommand::ResetLayout => {
                                                view.dock.reset(window, cx)
                                            }
                                        }
                                    }
                                });
                            }),
                    )
                })
        });
    }
    pub(super) fn save_layout(&mut self, cx: &mut Context<Self>) {
        self.message = match self.dock.save(cx) {
            Ok(true) => "Layout saved".into(),
            Ok(false) => "Layout persistence is disabled".into(),
            Err(error) => format!("Could not save layout: {error}"),
        };
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn confirmation_rechecks_availability_and_toggle_state() {
        let initial = Availability {
            loaded: true,
            undo: true,
            ..Default::default()
        };
        let items = entries(&initial);
        let undo = items
            .iter()
            .position(|entry| entry.label == "Undo")
            .unwrap();
        assert!(matches!(
            resolve(undo, &initial),
            Some(EditorCommand::Viewer(Action::Undo))
        ));
        assert!(resolve(undo, &Availability::default()).is_none());
        let snap = items
            .iter()
            .position(|entry| entry.label == "Toggle snapping")
            .unwrap();
        assert!(resolve(snap, &Availability::default()).is_none());
        let current = Availability {
            ready: true,
            snap: true,
            ..Default::default()
        };
        assert!(matches!(
            resolve(snap, &current),
            Some(EditorCommand::Viewer(Action::SetSnap(false)))
        ));
        assert!(resolve(usize::MAX, &current).is_none());
    }
}
