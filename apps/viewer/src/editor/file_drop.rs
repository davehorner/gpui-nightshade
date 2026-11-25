use std::path::{Path, PathBuf};

/// Trunk's native open-path action accepts one asset and preserves sibling paths
/// for external glTF buffers/textures. Do not reduce a drop to detached bytes.
pub(super) fn asset_path(paths: &[PathBuf]) -> Result<String, &'static str> {
    let [path] = paths else {
        return Err("Drop one model or HDR sky at a time");
    };
    if !path.is_absolute() {
        return Err("Dropped assets must have an absolute file path");
    }
    if !supported(path) {
        return Err("Drop a GLB, glTF, HDR, ZIP or FBX file");
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or("The dropped file path cannot be represented by the viewer")
}

fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["glb", "gltf", "hdr", "zip", "fbx"]
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

#[cfg(test)]
mod tests {
    use super::asset_path;
    use std::path::PathBuf;

    #[test]
    fn accepts_single_supported_paths_without_changing_unicode_or_sibling_location() {
        let root = std::env::temp_dir();
        for extension in ["GLB", "gltf", "HDR", "zip", "FBX"] {
            let path = root
                .join("模型 with spaces")
                .join(format!("asset.{extension}"));
            assert_eq!(
                asset_path(std::slice::from_ref(&path)).unwrap(),
                path.to_str().unwrap()
            );
        }
    }

    #[test]
    fn rejects_ambiguous_and_unsupported_drops() {
        let model = std::env::temp_dir().join("asset.glb");
        assert!(asset_path(&[]).is_err());
        assert!(asset_path(&[model.clone(), model]).is_err());
        assert!(asset_path(&[PathBuf::from("relative.glb")]).is_err());
        for name in ["image.png", "asset.glb.exe", "folder", "scene.obj"] {
            assert!(asset_path(&[std::env::temp_dir().join(name)]).is_err());
        }
    }
}
