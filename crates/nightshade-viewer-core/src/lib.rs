//! Native trunk viewer schedules with bounded frontend action/state exchange.
use gpui_nightshade::{
    host::{CameraPose, ViewportHost, WgpuRenderer},
    wgpu,
};
use nightshade::World;
use nightshade_window::{Host, Renderer};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
pub use viewer::core::*;

mod views;

const FRONTEND_CAPACITY: usize = 256;
#[derive(Clone, Default)]
pub struct ViewerConnection {
    actions: Arc<Mutex<VecDeque<ViewerAction>>>,
    latest: Arc<Mutex<Option<Arc<ViewerSnapshot>>>>,
}
impl ViewerConnection {
    pub fn submit(&self, action: ViewerAction) -> Result<(), ViewerAction> {
        let mut queue = self.actions.lock().unwrap();
        if queue.len() >= FRONTEND_CAPACITY {
            return Err(action);
        }
        queue.push_back(action);
        Ok(())
    }
    pub fn snapshot(&self) -> Option<Arc<ViewerSnapshot>> {
        self.latest.lock().unwrap().clone()
    }
}

pub struct ViewerRuntime {
    pub world: World,
    stages: nightshade_wgpu::commands::StageState,
    frame: nightshade_wgpu::WgpuFrameRenderer,
    connection: ViewerConnection,
    pick_pending: bool,
    views: views::SharedViews,
    reported_selection: Option<Option<u32>>,
    // Log guards and other plugin-owned objects must outlive all schedules/renderer work.
    _kept_alive: Vec<Box<dyn std::any::Any>>,
}
impl ViewerRuntime {
    pub async fn new(
        arguments: ViewerArguments,
        backends: wgpu::Backends,
        width: u32,
        height: u32,
        connection: ViewerConnection,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_options(
            arguments,
            backends,
            width,
            height,
            connection,
            if std::env::var_os("GPUI_NIGHTSHADE_BENCHMARK").is_some() {
                ViewerCorePlugin {
                    persist_settings: false,
                    remote_catalogs: false,
                }
            } else {
                ViewerCorePlugin::default()
            },
        )
        .await
    }
    pub async fn with_options(
        arguments: ViewerArguments,
        backends: wgpu::Backends,
        width: u32,
        height: u32,
        connection: ViewerConnection,
        plugin: ViewerCorePlugin,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut app = viewer::core::compose(arguments, plugin);
        let kept_alive = app.take_kept_alive();
        let (mut world, mut stages, hooks) = nightshade_wgpu::commands::into_parts(app);
        let renderer = WgpuRenderer::new_headless_with_backends(width, height, backends).await?;
        let mut frame = nightshade_wgpu::WgpuFrameRenderer::new(renderer, hooks);
        nightshade_window::run::initialize_offscreen(
            &mut world,
            &mut stages,
            &mut frame,
            (width, height),
            1.,
        );
        let mut runtime = Self {
            world,
            stages,
            frame,
            connection,
            pick_pending: false,
            views: views::SharedViews {
                width,
                height,
                ..Default::default()
            },
            reported_selection: None,
            _kept_alive: kept_alive,
        };
        runtime
            .world
            .res_mut::<viewer::core::FrontendInput>()
            .pointer_over_ui = true;
        configure_editor_camera(&mut runtime.world);
        runtime
            .views
            .configure(&mut runtime.world, Default::default(), 0);
        runtime.render()?;
        Ok(runtime)
    }
    fn advance_scene(&mut self) {
        {
            let mut queue = self.connection.actions.lock().unwrap();
            while let Some(action) = queue.pop_front() {
                if let Err(action) = viewer::core::submit(&mut self.world, action) {
                    queue.push_front(action);
                    break;
                }
            }
        }
        nightshade_window::run::advance_timing(&mut self.world.resources);
        self.world.increment_tick();
        self.stages.run_systems(&mut self.world);
        self.views.after_scene(&mut self.world);
        self.views.apply_requests(&mut self.world);
    }
    fn draw_frame(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.frame
            .update(&self.world.storage, &mut self.world.resources)
            .map_err(|e| e.0)?;
        self.frame
            .prepare(&mut self.world.storage, &mut self.world.resources);
        self.frame.render(&mut self.world).map_err(|e| e.0)?;
        Ok(())
    }
    fn publish(&self) {
        *self.connection.latest.lock().unwrap() =
            Some(Arc::new(viewer::core::snapshot(&self.world)));
    }
}
impl ViewportHost for ViewerRuntime {
    fn renderer(&self) -> &WgpuRenderer {
        &self.frame.renderer
    }
    fn renderer_mut(&mut self) -> &mut WgpuRenderer {
        &mut self.frame.renderer
    }
    fn render(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.advance_scene();
        self.draw_frame()?;
        self.publish();
        Ok(())
    }
    fn service_hidden(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.advance_scene();
        // Explicit screenshot/texture/sky jobs must still complete when tabbed away.
        if !self
            .world
            .res::<nightshade_graphics::resources::queues::CommandQueues>()
            .render
            .is_empty()
            || self.frame.renderer.screenshot_pending()
        {
            self.draw_frame()?;
        }
        self.publish();
        Ok(())
    }
    fn set_camera(&mut self, pose: CameraPose) {
        let previous = self.views.poses[self.views.active].replace(pose);
        let Some(previous) = previous else {
            return;
        };
        let camera = self
            .world
            .res::<nightshade_camera::resources::ActiveCamera>()
            .0;
        if let Some(camera) = camera
            && let Some(orbit) =
                self.world
                    .storage
                    .get_mut::<nightshade_camera_controller::contract::PanOrbitCamera>(camera)
        {
            orbit.target_yaw += pose.yaw - previous.yaw;
            orbit.target_pitch += pose.pitch - previous.pitch;
            orbit.target_radius =
                (orbit.target_radius * pose.distance / previous.distance.max(0.001)).max(0.001);
            orbit.target_focus += nightshade::prelude::nalgebra_glm::Vec3::from(pose.target)
                - nightshade::prelude::nalgebra_glm::Vec3::from(previous.target);
        }
    }
    fn camera_layout_state(&self) -> Option<(gpui_nightshade::views::CameraLayout, usize)> {
        Some((self.views.layout, self.views.active))
    }
    fn set_camera_layout(
        &mut self,
        layout: gpui_nightshade::views::CameraLayout,
        active: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.views.configure(&mut self.world, layout, active);
        Ok(())
    }
    fn pointer_input(&mut self, event: gpui_nightshade::input::PointerInput) {
        apply_pointer_input(&mut self.world, event);
    }
    fn request_pick(&mut self, x: u32, y: u32) {
        viewer::core::request_pick(&mut self.world, x, y);
        self.pick_pending = true;
    }
    fn poll_pick(&mut self) -> Option<Option<u32>> {
        let selected = self
            .connection
            .snapshot()
            .and_then(|s| s.selection.selected.map(|e| e.id));
        if (self.pick_pending && !viewer::core::pick_pending(&self.world))
            || self.reported_selection != Some(selected)
        {
            self.pick_pending = false;
            self.reported_selection = Some(selected);
            Some(selected)
        } else {
            None
        }
    }
    fn resize(&mut self, width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.frame.renderer.set_owned_output(width, height)?;
        self.views.width = width;
        self.views.height = height;
        self.views.publish_layout(&mut self.world);
        self.world
            .res_mut::<nightshade_platform::contract::window::Window>()
            .cached_viewport_size = Some((width, height));
        Ok(())
    }
    fn continuous(&self) -> bool {
        true
    } // Loading, animation, smoothing and native MCP all require schedule service.
}
pub fn open(
    arguments: ViewerArguments,
    backends: wgpu::Backends,
    width: u32,
    height: u32,
    connection: ViewerConnection,
) -> Result<Box<dyn ViewportHost>, Box<dyn std::error::Error>> {
    Ok(Box::new(pollster::block_on(ViewerRuntime::new(
        arguments, backends, width, height, connection,
    ))?))
}

fn apply_pointer_input(world: &mut World, event: gpui_nightshade::input::PointerInput) {
    use gpui_nightshade::input::PointerPhase;
    use nightshade_platform::winit::{
        event::{ElementState, MouseButton},
        keyboard::KeyCode,
    };
    if event.phase == PointerPhase::Cancel {
        viewer::core::cancel_pointer_gesture(world);
    }
    let inactive = matches!(event.phase, PointerPhase::Leave | PointerPhase::Cancel);
    let outside = inactive
        || world
            .res::<nightshade_platform::contract::window::Window>()
            .cached_viewport_size
            .is_none_or(|(w, h)| {
                event.position[0] < 0.
                    || event.position[1] < 0.
                    || event.position[0] >= w as f32
                    || event.position[1] >= h as f32
            });
    world
        .res_mut::<viewer::core::FrontendInput>()
        .pointer_over_ui = outside;
    let resources = &mut world.resources;
    if event.position.iter().all(|v| v.is_finite()) {
        nightshade_window::run::input_inject_cursor_moved(
            resources,
            nightshade::prelude::nalgebra_glm::vec2(event.position[0], event.position[1]),
        );
    }
    let keyboard = &mut resources
        .res_mut::<nightshade_platform::contract::input::Input>()
        .keyboard;
    for (key, held) in [
        (KeyCode::ControlLeft, event.control && !inactive),
        (KeyCode::ShiftLeft, event.shift && !inactive),
    ] {
        keyboard.keystates.insert(
            key,
            if held {
                ElementState::Pressed
            } else {
                ElementState::Released
            },
        );
    }
    match event.phase {
        PointerPhase::Press => nightshade_window::run::input_inject_mouse_button(
            resources,
            MouseButton::Left,
            ElementState::Pressed,
        ),
        PointerPhase::Release | PointerPhase::Cancel => {
            nightshade_window::run::input_inject_mouse_button(
                resources,
                MouseButton::Left,
                ElementState::Released,
            )
        }
        _ => {}
    }
}

fn configure_editor_camera(world: &mut World) {
    if let Some(camera) = world.res::<nightshade_camera::resources::ActiveCamera>().0
        && let Some(orbit) = world
            .storage
            .get_mut::<nightshade_camera_controller::contract::PanOrbitCamera>(camera)
    {
        use nightshade_camera_controller::contract::PanOrbitButton;
        orbit.bindings.orbit = PanOrbitButton::Right;
        orbit.bindings.pan = PanOrbitButton::Middle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_edges_drive_native_gizmo_and_undo_without_moving_camera() {
        use gpui_nightshade::input::{PointerInput, PointerPhase};
        let mut app = viewer::core::compose(
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
        app.world
            .res_mut::<nightshade_platform::contract::window::Window>()
            .cached_viewport_size = Some((640, 480));
        app.stages.apply_orderings();
        app.startup.run(&mut app.world);
        configure_editor_camera(&mut app.world);
        viewer::core::submit(
            &mut app.world,
            ViewerAction::AddPrimitive(PrimitiveKind::Cube),
        )
        .unwrap();
        for _ in 0..3 {
            app.world.increment_tick();
            app.stages.run(&mut app.world);
        }
        let initial = viewer::core::snapshot(&app.world);
        let camera = app
            .world
            .res::<nightshade_camera::resources::ActiveCamera>()
            .0
            .unwrap();
        let orbit = app
            .world
            .storage
            .get::<nightshade_camera_controller::contract::PanOrbitCamera>(camera)
            .unwrap();
        let before = (
            orbit.target_yaw,
            orbit.target_pitch,
            orbit.target_radius,
            orbit.target_focus,
        );
        for (phase, position) in [
            (PointerPhase::Press, [320., 240.]),
            (PointerPhase::Move, [360., 260.]),
            (PointerPhase::Release, [360., 260.]),
        ] {
            apply_pointer_input(
                &mut app.world,
                PointerInput {
                    phase,
                    position,
                    control: false,
                    shift: false,
                },
            );
            app.world.increment_tick();
            app.stages.run(&mut app.world);
        }
        let edited = viewer::core::snapshot(&app.world);
        assert_ne!(
            edited.selection.translation, initial.selection.translation,
            "the center gizmo must move the selected cube"
        );
        assert!(edited.can_undo);
        let orbit = app
            .world
            .storage
            .get::<nightshade_camera_controller::contract::PanOrbitCamera>(camera)
            .unwrap();
        assert_eq!(
            (
                orbit.target_yaw,
                orbit.target_pitch,
                orbit.target_radius,
                orbit.target_focus
            ),
            before
        );
        viewer::core::submit(&mut app.world, ViewerAction::Undo).unwrap();
        app.world.increment_tick();
        app.stages.run(&mut app.world);
        assert_eq!(
            viewer::core::snapshot(&app.world).selection.translation,
            initial.selection.translation
        );
        for (phase, position) in [
            (PointerPhase::Press, [320., 240.]),
            (PointerPhase::Move, [360., 260.]),
            (PointerPhase::Cancel, [0., 0.]),
        ] {
            apply_pointer_input(
                &mut app.world,
                PointerInput {
                    phase,
                    position,
                    control: false,
                    shift: false,
                },
            );
            app.world.increment_tick();
            app.stages.run(&mut app.world);
        }
        let cancelled = viewer::core::snapshot(&app.world);
        assert_eq!(
            cancelled.selection.translation, initial.selection.translation,
            "cancel restores the drag start"
        );
        assert!(!cancelled.can_undo, "cancel must not add a history entry");
        apply_pointer_input(
            &mut app.world,
            PointerInput {
                phase: PointerPhase::Cancel,
                position: [0.; 2],
                control: true,
                shift: true,
            },
        );
        let input = app
            .world
            .res::<nightshade_platform::contract::input::Input>();
        assert!(
            !input
                .mouse
                .state
                .contains(nightshade_platform::contract::input::MouseState::LEFT_CLICKED)
        );
        assert!(
            !input
                .keyboard
                .is_key_pressed(nightshade_platform::winit::keyboard::KeyCode::ControlLeft)
        );
        assert!(
            app.world
                .res::<viewer::core::FrontendInput>()
                .pointer_over_ui
        );
    }
    #[test]
    fn bridge_bounds_pending_actions_and_preserves_fifo() {
        let connection = ViewerConnection::default();
        for index in 0..FRONTEND_CAPACITY {
            assert!(
                connection
                    .submit(ViewerAction::Rename(index.to_string()))
                    .is_ok()
            );
        }
        assert!(connection.submit(ViewerAction::Undo).is_err());
        let mut queue = connection.actions.lock().unwrap();
        for index in 0..FRONTEND_CAPACITY {
            assert!(
                matches!(queue.pop_front(),Some(ViewerAction::Rename(name)) if name==index.to_string())
            );
        }
        assert!(queue.is_empty());
        assert!(connection.snapshot().is_none());
    }
}
