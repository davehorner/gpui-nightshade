use gpui_nightshade::{host::CameraPose, views::CameraLayout};
use nightshade::{World, ecs::Entity};
use nightshade_camera::resources::ActiveCamera;
use nightshade_camera_controller::contract::{PanOrbitCamera, compute_pan_orbit_transform};
use nightshade_graphics::prelude::{Viewport, ViewportRect};
use nightshade_scene::contract::{GlobalTransform, LocalTransform, Name};

#[derive(Default)]
pub(crate) struct SharedViews {
    pub layout: CameraLayout,
    pub active: usize,
    pub cameras: [Option<Entity>; 4],
    pub poses: [Option<CameraPose>; 4],
    pub width: u32,
    pub height: u32,
}

impl SharedViews {
    pub fn configure(&mut self, world: &mut World, layout: CameraLayout, active: usize) {
        let count = layout.rects(self.width, self.height).len();
        let active = active.min(count - 1);
        if self.layout != layout || self.active != active {
            viewer::core::cancel_pointer_gesture(world);
        }
        self.layout = layout;
        self.active = active;
        if self.cameras.iter().all(Option::is_none) {
            self.cameras[0] = viewer::core::editor_camera(world)
                .filter(|e| world.storage.is_alive(*e))
                .or(world.res::<ActiveCamera>().0);
            self.poses[0] = None;
        }
        self.ensure_cameras(world, count);
        world.res_mut::<ActiveCamera>().0 = self.cameras[self.active];
        self.publish_layout(world);
    }

    fn ensure_cameras(&mut self, world: &mut World, count: usize) {
        let Some(primary) = self
            .cameras
            .iter()
            .flatten()
            .copied()
            .find(|e| {
                world.storage.is_alive(*e)
                    && world
                        .storage
                        .has::<nightshade_camera::components::Camera>(*e)
            })
            .or_else(|| viewer::core::editor_camera(world))
        else {
            return;
        };
        let camera = world
            .storage
            .get::<nightshade_camera::components::Camera>(primary)
            .copied()
            .unwrap_or_default();
        let base = world
            .storage
            .get::<PanOrbitCamera>(primary)
            .copied()
            .unwrap_or_default();
        for (i, slot) in self.cameras.iter_mut().enumerate().take(count) {
            if slot.is_some_and(|e| {
                world.storage.is_alive(e)
                    && world
                        .storage
                        .has::<nightshade_camera::components::Camera>(e)
            }) {
                continue;
            }
            let mut orbit = base;
            orbit.yaw = base.yaw + i as f32 * std::f32::consts::FRAC_PI_2;
            orbit.target_yaw = orbit.yaw;
            let (translation, rotation) =
                compute_pan_orbit_transform(orbit.focus, orbit.yaw, orbit.pitch, orbit.radius);
            *slot = Some(world.storage.spawn((
                camera,
                orbit,
                LocalTransform {
                    translation,
                    rotation,
                    ..Default::default()
                },
                GlobalTransform::default(),
                Name(format!("Editor camera {}", i + 1)),
            )));
            self.poses[i] = None;
        }
    }

    pub fn after_scene(&mut self, world: &mut World) {
        // Camera activation through the inspector/MCP binds the focused view.
        if let Some(actual) = world.res::<ActiveCamera>().0
            && Some(actual) != self.cameras[self.active]
        {
            if let Some(other) = self.cameras.iter().position(|e| *e == Some(actual)) {
                self.cameras.swap(self.active, other);
                self.poses[other] = None;
            } else {
                self.cameras[self.active] = Some(actual);
            }
            self.poses[self.active] = None;
        }
        self.ensure_cameras(world, self.layout.rects(self.width, self.height).len());
        world.res_mut::<ActiveCamera>().0 = self.cameras[self.active];
        // Expose managed camera IDs through the existing camera snapshot/MCP tool.
        for (i, entity) in self.cameras.iter().enumerate() {
            if let Some(entity) = entity {
                viewer::core::register_frontend_camera(
                    world,
                    *entity,
                    format!("Viewport camera {}", i + 1),
                );
            }
        }
        self.publish_layout(world);
        self.publish_state(world);
    }

    pub fn apply_requests(&mut self, world: &mut World) {
        for request in viewer::core::take_camera_view_requests(world) {
            let result = self.apply_request(world, request.command);
            self.after_scene(world);
            #[cfg(not(target_arch = "wasm32"))]
            viewer::core::finish_camera_view_request(world, &request.operation_id, result);
            #[cfg(target_arch = "wasm32")]
            let _ = result;
        }
    }

    fn apply_request(
        &mut self,
        world: &mut World,
        command: viewer::core::CameraViewCommand,
    ) -> Result<(), String> {
        use viewer::core::CameraViewCommand;
        match command {
            CameraViewCommand::Layout { count, active } => {
                let layout = match count {
                    1 => CameraLayout::Single,
                    2 => CameraLayout::Split,
                    4 => CameraLayout::Quad,
                    _ => return Err("camera count must be 1, 2 or 4".into()),
                };
                let visible = layout.rects(self.width, self.height).len();
                if active.is_some_and(|i| i >= visible) {
                    return Err("active tile is unavailable at this output size".into());
                }
                self.configure(
                    world,
                    layout,
                    active.unwrap_or(self.active.min(visible - 1)),
                );
            }
            CameraViewCommand::Focus { index } => {
                self.check_index(index)?;
                self.configure(world, self.layout, index);
            }
            CameraViewCommand::Assign { index, camera } => {
                self.check_index(index)?;
                let camera = camera
                    .or_else(|| viewer::core::editor_camera(world))
                    .ok_or("the editor camera is unavailable")?;
                if !world.storage.is_alive(camera)
                    || !world
                        .storage
                        .has::<nightshade_camera::components::Camera>(camera)
                {
                    return Err("the camera target no longer exists".into());
                }
                viewer::core::cancel_pointer_gesture(world);
                if let Some(other) = self.cameras.iter().position(|e| *e == Some(camera)) {
                    self.cameras.swap(index, other);
                    self.poses[other] = None;
                } else {
                    self.cameras[index] = Some(camera);
                }
                self.poses[index] = None;
                world.res_mut::<ActiveCamera>().0 = self.cameras[self.active];
            }
        }
        Ok(())
    }

    fn check_index(&self, index: usize) -> Result<(), String> {
        if index >= self.layout.rects(self.width, self.height).len() {
            return Err("there is no visible tile at that index".into());
        }
        Ok(())
    }

    fn publish_state(&self, world: &mut World) {
        let count = match self.layout {
            CameraLayout::Single => 1,
            CameraLayout::Split => 2,
            CameraLayout::Quad => 4,
        };
        let tiles = self
            .layout
            .rects(self.width, self.height)
            .iter()
            .enumerate()
            .filter_map(|(index, r)| {
                self.cameras[index].map(|camera| viewer::core::CameraTile {
                    index,
                    camera,
                    rect: [r.x, r.y, r.width, r.height],
                })
            })
            .collect();
        viewer::core::publish_camera_views(
            world,
            viewer::core::CameraViews {
                supported: true,
                count,
                active: self.active,
                width: self.width,
                height: self.height,
                tiles,
            },
        );
    }

    pub fn publish_layout(&self, world: &mut World) {
        let rects = self.layout.rects(self.width, self.height);
        let viewport = world.res_mut::<Viewport>();
        viewport.required_cameras.clear();
        viewport.camera_tile_rects.clear();
        viewport.active_viewport_rect = None;
        for camera in self.cameras.iter().flatten() {
            viewport.force_render_cameras.remove(camera);
        }
        if rects.len() == 1 {
            return;
        }
        for (i, rect) in rects.iter().enumerate() {
            if let Some(camera) = self.cameras[i] {
                let rect = ViewportRect {
                    x: rect.x as f32,
                    y: rect.y as f32,
                    width: rect.width as f32,
                    height: rect.height as f32,
                };
                // Morph weights and GPU skinning can change without dirty world
                // bounds. All displayed cameras must sample this simulation frame.
                viewport.force_render_cameras.insert(camera);
                viewport.required_cameras.push(camera);
                viewport.camera_tile_rects.insert(camera, rect);
                if i == self.active {
                    viewport.active_viewport_rect = Some(rect);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_camera_commands_share_state_swap_assignments_and_reject_stale_targets() {
        use viewer::core::CameraViewCommand as Command;
        let mut app = viewer::core::compose(
            viewer::core::ViewerArguments {
                no_agent: true,
                preview: true,
                ..Default::default()
            },
            viewer::core::ViewerCorePlugin {
                persist_settings: false,
                remote_catalogs: false,
            },
        );
        app.stages.apply_orderings();
        app.startup.run(&mut app.world);
        let mut views = SharedViews {
            width: 801,
            height: 601,
            ..Default::default()
        };
        views
            .apply_request(
                &mut app.world,
                Command::Layout {
                    count: 4,
                    active: Some(2),
                },
            )
            .unwrap();
        views.after_scene(&mut app.world);
        let initial = viewer::core::camera_views(&app.world);
        assert_eq!(initial.tiles.len(), 4);
        assert_eq!(initial.active, 2);
        let cameras = views.cameras;
        views
            .apply_request(
                &mut app.world,
                Command::Assign {
                    index: 2,
                    camera: cameras[0],
                },
            )
            .unwrap();
        views.after_scene(&mut app.world);
        assert_eq!(views.cameras[2], cameras[0]);
        assert_eq!(views.cameras[0], cameras[2]);
        assert_eq!(app.world.res::<ActiveCamera>().0, cameras[0]);
        let imported = app.world.storage.spawn((
            nightshade_camera::components::Camera::default(),
            LocalTransform::default(),
            GlobalTransform::default(),
        ));
        app.world.storage.despawn_entities(&[imported]);
        // A model-owned camera may disappear when another model is loaded.
        let imported_live = app.world.storage.spawn((
            nightshade_camera::components::Camera::default(),
            LocalTransform::default(),
            GlobalTransform::default(),
        ));
        views
            .apply_request(
                &mut app.world,
                Command::Assign {
                    index: 0,
                    camera: Some(imported_live),
                },
            )
            .unwrap();
        app.world.storage.despawn_entities(&[imported_live]);
        views.after_scene(&mut app.world);
        let repaired = viewer::core::camera_views(&app.world);
        assert!(
            repaired
                .tiles
                .iter()
                .all(|t| app.world.storage.is_alive(t.camera))
        );
        assert_eq!(
            repaired
                .tiles
                .iter()
                .map(|t| t.camera)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            4
        );
        let before = views.cameras;
        assert!(
            views
                .apply_request(
                    &mut app.world,
                    Command::Assign {
                        index: 1,
                        camera: Some(imported)
                    }
                )
                .is_err()
        );
        assert_eq!(views.cameras, before);
        views
            .apply_request(
                &mut app.world,
                Command::Layout {
                    count: 2,
                    active: None,
                },
            )
            .unwrap();
        views
            .apply_request(&mut app.world, Command::Focus { index: 1 })
            .unwrap();
        views.after_scene(&mut app.world);
        let current = viewer::core::camera_views(&app.world);
        assert_eq!(current.tiles.len(), 2);
        assert_eq!(current.active, 1);
        assert_eq!(current.tiles[1].rect, [400, 0, 401, 601]);
        assert_eq!(
            initial.tiles.len(),
            4,
            "snapshots retain their original layout"
        );
        assert!(
            views
                .apply_request(&mut app.world, Command::Focus { index: 3 })
                .is_err()
        );
    }
    #[test]
    fn four_cameras_share_one_scene_and_survive_focus_and_layout_changes() {
        let mut app = viewer::core::compose(
            viewer::core::ViewerArguments {
                no_agent: true,
                preview: true,
                ..Default::default()
            },
            viewer::core::ViewerCorePlugin {
                persist_settings: false,
                remote_catalogs: false,
            },
        );
        app.stages.apply_orderings();
        app.startup.run(&mut app.world);
        let mut views = SharedViews {
            width: 801,
            height: 601,
            ..Default::default()
        };
        views.configure(&mut app.world, CameraLayout::Quad, 2);
        let cameras = views.cameras;
        assert_eq!(app.world.res::<Viewport>().required_cameras.len(), 4);
        assert_eq!(app.world.res::<ActiveCamera>().0, cameras[2]);
        viewer::core::submit(
            &mut app.world,
            viewer::core::ViewerAction::AddPrimitive(viewer::core::PrimitiveKind::Cube),
        )
        .unwrap();
        for _ in 0..3 {
            app.world.increment_tick();
            app.stages.run(&mut app.world);
        }
        let nodes = viewer::core::snapshot(&app.world).nodes;
        views.configure(&mut app.world, CameraLayout::Split, 1);
        views.configure(&mut app.world, CameraLayout::Single, 0);
        views.configure(&mut app.world, CameraLayout::Quad, 3);
        assert_eq!(views.cameras, cameras);
        assert_eq!(viewer::core::snapshot(&app.world).nodes.len(), nodes.len());
        assert_eq!(app.world.res::<ActiveCamera>().0, cameras[3]);
    }
    #[test]
    #[ignore = "requires a native GPU adapter"]
    fn native_shared_views_keep_camera_focus_and_pick_the_same_entity() {
        use gpui_nightshade::host::ViewportHost;
        let mut runtime = pollster::block_on(crate::ViewerRuntime::with_options(
            viewer::core::ViewerArguments {
                no_agent: true,
                preview: true,
                ..Default::default()
            },
            gpui_nightshade::wgpu::Backends::DX12,
            801,
            601,
            crate::ViewerConnection::default(),
            viewer::core::ViewerCorePlugin {
                persist_settings: false,
                remote_catalogs: false,
            },
        ))
        .unwrap();
        viewer::core::submit(
            &mut runtime.world,
            viewer::core::ViewerAction::AddPrimitive(viewer::core::PrimitiveKind::Cube),
        )
        .unwrap();
        for _ in 0..3 {
            runtime.render().unwrap();
        }
        let entity = viewer::core::snapshot(&runtime.world)
            .selection
            .selected
            .unwrap();
        runtime.set_camera_layout(CameraLayout::Quad, 1).unwrap();
        let cameras = runtime.views.cameras;
        for _ in 0..4 {
            runtime.render().unwrap();
            assert_eq!(runtime.world.res::<ActiveCamera>().0, cameras[1]);
            assert_eq!(runtime.views.cameras, cameras);
        }
        assert_eq!(runtime.frame.renderer.camera_viewports.len(), 4);
        let before = *runtime
            .world
            .storage
            .get::<LocalTransform>(cameras[0].unwrap())
            .unwrap();
        let moved_before = *runtime
            .world
            .storage
            .get::<LocalTransform>(cameras[1].unwrap())
            .unwrap();
        runtime.set_camera(CameraPose {
            yaw: 0.6,
            pitch: 0.35,
            distance: 4.,
            target: [0.; 3],
        });
        runtime.set_camera(CameraPose {
            yaw: 0.9,
            pitch: 0.35,
            distance: 4.,
            target: [0.; 3],
        });
        for _ in 0..10 {
            runtime.render().unwrap();
        }
        assert_eq!(
            *runtime
                .world
                .storage
                .get::<LocalTransform>(cameras[0].unwrap())
                .unwrap(),
            before
        );
        assert_ne!(
            *runtime
                .world
                .storage
                .get::<LocalTransform>(cameras[1].unwrap())
                .unwrap(),
            moved_before
        );
        runtime.request_pick(600, 150);
        for _ in 0..80 {
            runtime.render().unwrap();
            runtime
                .frame
                .renderer
                .device
                .poll(gpui_nightshade::wgpu::PollType::Poll)
                .unwrap();
            if !viewer::core::pick_pending(&runtime.world) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!viewer::core::pick_pending(&runtime.world));
        assert_eq!(
            viewer::core::snapshot(&runtime.world).selection.selected,
            Some(entity)
        );
    }
    #[test]
    #[ignore = "requires a native GPU adapter"]
    fn morph_deformation_changes_pixels_in_every_shared_camera() {
        use gpui_nightshade::{
            host::ViewportHost,
            presentation::{AlphaMode, readback::ReadbackRing},
        };
        use nightshade_graphics::prelude::{Atmosphere, MorphWeights, RenderSettings};
        fn capture(runtime: &crate::ViewerRuntime) -> Vec<u8> {
            let renderer = &runtime.frame.renderer;
            let source = renderer.external_output.as_ref().unwrap();
            let mut ring = ReadbackRing::new(
                &renderer.device,
                640,
                480,
                0,
                renderer.surface_format,
                AlphaMode::Opaque,
                2,
            )
            .unwrap();
            ring.enqueue(&renderer.device, &renderer.queue, source, 0)
                .unwrap();
            renderer
                .device
                .poll(gpui_nightshade::wgpu::PollType::wait_indefinitely())
                .unwrap();
            ring.take_latest().unwrap().rgba
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/mcp-cinematic/assets/AnimatedMorphCube.glb");
        let mut runtime = pollster::block_on(crate::ViewerRuntime::with_options(
            viewer::core::ViewerArguments {
                no_agent: true,
                preview: true,
                model: Some(path.to_string_lossy().into_owned()),
                ..Default::default()
            },
            gpui_nightshade::wgpu::Backends::DX12,
            640,
            480,
            crate::ViewerConnection::default(),
            viewer::core::ViewerCorePlugin {
                persist_settings: false,
                remote_catalogs: false,
            },
        ))
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let entity = loop {
            runtime.render().unwrap();
            if let Some(morph) = viewer::core::snapshot(&runtime.world).morphs.first() {
                break morph.entity;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        viewer::core::submit(
            &mut runtime.world,
            viewer::core::ViewerAction::StopAnimation,
        )
        .unwrap();
        runtime.render().unwrap();
        {
            let settings = runtime.world.res_mut::<RenderSettings>();
            settings.show_sky = false;
            settings.atmosphere = Atmosphere::None;
        }
        runtime.set_camera_layout(CameraLayout::Quad, 0).unwrap();
        runtime
            .world
            .storage
            .get_mut::<MorphWeights>(entity)
            .unwrap()
            .weights = vec![0., 0.];
        for _ in 0..3 {
            runtime.render().unwrap();
        }
        let before = capture(&runtime);
        runtime
            .world
            .storage
            .get_mut::<MorphWeights>(entity)
            .unwrap()
            .weights = vec![1., 0.];
        for _ in 0..3 {
            runtime.render().unwrap();
        }
        assert_eq!(
            runtime
                .world
                .storage
                .get::<MorphWeights>(entity)
                .unwrap()
                .weights,
            vec![1., 0.],
            "source weights were reset"
        );
        let rendered = runtime
            .world
            .storage
            .get::<nightshade_render::components::render_world::MorphWeights>(entity)
            .unwrap()
            .0;
        assert_eq!(
            &rendered[..2],
            &[1., 0.],
            "render extraction lost morph weights"
        );
        let after = capture(&runtime);
        fn assert_changed(before: &[u8], after: &[u8]) {
            for rect in CameraLayout::Quad.rects(640, 480) {
                let mut changed = 0;
                for y in rect.y..rect.y + rect.height {
                    for x in rect.x..rect.x + rect.width {
                        let offset = ((y * 640 + x) * 4) as usize;
                        if (0..3).any(|c| before[offset + c].abs_diff(after[offset + c]) > 8) {
                            changed += 1;
                        }
                    }
                }
                assert!(
                    changed > 50,
                    "morph changed only {changed} pixels in {rect:?}"
                );
            }
        }
        assert_changed(&before, &after);
        for action in [
            viewer::core::ViewerAction::SetBlendTime(0.),
            viewer::core::ViewerAction::SetAnimationLoop(true),
            viewer::core::ViewerAction::BlendToClip(0),
        ] {
            viewer::core::submit(&mut runtime.world, action).unwrap();
        }
        runtime.render().unwrap();
        let first_time = viewer::core::snapshot(&runtime.world).animation.time;
        let playing_before = capture(&runtime);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while viewer::core::snapshot(&runtime.world).animation.time < first_time + 0.8 {
            runtime.render().unwrap();
            assert!(
                std::time::Instant::now() < deadline,
                "native clip did not advance"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let playing_after = capture(&runtime);
        assert_changed(&playing_before, &playing_after);
    }
}
