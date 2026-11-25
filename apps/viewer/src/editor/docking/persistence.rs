use gpui_kit::component::dock::{DockAreaState, DockPlacement, PanelInfo, PanelState};
use gpui_nightshade_components::gpui_kit;
use std::{
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 64 * 1024;
const PANELS: [&str; 4] = ["Scene", "Viewport", "Inspector", "Animation"];

pub(super) fn layout_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("GPUI_NIGHTSHADE_LAYOUT") {
        return (!path.is_empty()).then(|| PathBuf::from(path));
    }
    let root = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(root.join("gpui-nightshade").join("layout.json"))
}

fn validate(state: &DockAreaState) -> Result<(), String> {
    if state.version != Some(1) {
        return Err("unsupported layout version".into());
    }
    let mut names = HashSet::new();
    let mut count = 0;
    fn visit<'a>(
        node: &'a PanelState,
        depth: usize,
        count: &mut usize,
        names: &mut HashSet<&'a str>,
    ) -> Result<(), String> {
        *count += 1;
        if depth > 16 || *count > 64 {
            return Err("layout is too complex".into());
        }
        match &node.info {
            PanelInfo::Panel(_) => {
                if !node.children.is_empty()
                    || !PANELS.contains(&node.panel_name.as_str())
                    || !names.insert(&node.panel_name)
                {
                    return Err("layout contains unknown or repeated panels".into());
                }
            }
            PanelInfo::Tabs { active_index } => {
                if node.panel_name != "TabPanel"
                    || node.children.is_empty()
                    || *active_index >= node.children.len()
                    || node
                        .children
                        .iter()
                        .any(|c| !matches!(c.info, PanelInfo::Panel(_)))
                {
                    return Err("invalid tab group".into());
                }
            }
            PanelInfo::Stack { sizes, axis } => {
                if node.panel_name != "StackPanel"
                    || node.children.is_empty()
                    || *axis > 1
                    || sizes.len() != node.children.len()
                    || sizes.iter().any(|s| {
                        !f32::from(*s).is_finite() || f32::from(*s) < 0. || f32::from(*s) > 100_000.
                    })
                {
                    return Err("invalid split geometry".into());
                }
            }
        }
        for child in &node.children {
            visit(child, depth + 1, count, names)?;
        }
        Ok(())
    }
    visit(&state.center, 0, &mut count, &mut names)?;
    for (dock, placement) in [
        (&state.left_dock, DockPlacement::Left),
        (&state.right_dock, DockPlacement::Right),
        (&state.bottom_dock, DockPlacement::Bottom),
    ] {
        if let Some(dock) = dock {
            if dock.placement() != placement
                || !f32::from(dock.size()).is_finite()
                || f32::from(dock.size()) <= 0.
                || f32::from(dock.size()) > 100_000.
            {
                return Err("invalid dock geometry".into());
            }
            visit(dock.panel(), 0, &mut count, &mut names)?;
        }
    }
    if names.len() != PANELS.len() {
        return Err("layout is missing editor panels".into());
    }
    Ok(())
}

pub(super) fn read(path: &Path) -> Result<Option<DockAreaState>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("layout file is too large".into());
    }
    let state = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate(&state)?;
    Ok(Some(state))
}

pub(super) fn write(path: &Path, state: &DockAreaState) -> Result<(), String> {
    validate(state)?;
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("layout file is too large".into());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Axis, px};

    fn fixture() -> DockAreaState {
        DockAreaState {
            version: Some(1),
            center: PanelState {
                panel_name: "StackPanel".into(),
                children: vec![PanelState {
                    panel_name: "TabPanel".into(),
                    children: PANELS.iter().map(|name| PanelState::new(*name)).collect(),
                    info: PanelInfo::tabs(2),
                }],
                info: PanelInfo::stack(vec![px(900.)], Axis::Horizontal),
            },
            ..Default::default()
        }
    }

    #[test]
    fn rejects_missing_duplicate_unknown_and_incompatible_panels() {
        let base = fixture();
        assert!(validate(&base).is_ok());
        let mut state = base.clone();
        state.center.children[0].children.pop();
        assert!(validate(&state).is_err());
        for name in ["Scene", "Unknown"] {
            let mut state = base.clone();
            state.center.children[0].children[1].panel_name = name.into();
            assert!(validate(&state).is_err());
        }
        let mut state = base.clone();
        state.version = Some(99);
        assert!(validate(&state).is_err());
        let mut state = base.clone();
        state.center.info = PanelInfo::stack(vec![px(f32::NAN)], Axis::Horizontal);
        assert!(validate(&state).is_err());
        let mut state = base;
        state.center.children[0].info = PanelInfo::tabs(4);
        assert!(validate(&state).is_err());
    }

    #[test]
    fn file_roundtrip_replacement_and_corruption() {
        let path =
            std::env::temp_dir().join(format!("nightshade-layout-{}.json", std::process::id()));
        let state = fixture();
        write(&path, &state).unwrap();
        assert_eq!(read(&path).unwrap(), Some(state.clone()));
        let mut changed = state;
        changed.center.children[0].info = PanelInfo::tabs(0);
        write(&path, &changed).unwrap();
        assert_eq!(read(&path).unwrap(), Some(changed));
        std::fs::write(&path, b"{truncated").unwrap();
        assert!(read(&path).is_err());
        std::fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert!(read(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(read(&path).unwrap().is_none());
    }
}
