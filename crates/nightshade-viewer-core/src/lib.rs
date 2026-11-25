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
    last_pose: Option<CameraPose>,
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
            ViewerCorePlugin::default(),
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
            last_pose: None,
            _kept_alive: kept_alive,
        };
        runtime.render()?;
        Ok(runtime)
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
        self.frame
            .update(&self.world.storage, &mut self.world.resources)
            .map_err(|e| e.0)?;
        self.frame
            .prepare(&mut self.world.storage, &mut self.world.resources);
        self.frame.render(&mut self.world).map_err(|e| e.0)?;
        self.publish();
        Ok(())
    }
    fn set_camera(&mut self, pose: CameraPose) {
        let previous = self.last_pose.replace(pose);
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
    fn request_pick(&mut self, x: u32, y: u32) {
        viewer::core::request_pick(&mut self.world, x, y);
        self.pick_pending = true;
    }
    fn poll_pick(&mut self) -> Option<Option<u32>> {
        if self.pick_pending && !viewer::core::pick_pending(&self.world) {
            self.pick_pending = false;
            Some(
                viewer::core::snapshot(&self.world)
                    .selection
                    .selected
                    .map(|entity| entity.id),
            )
        } else {
            None
        }
    }
    fn resize(&mut self, width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.frame.renderer.set_owned_output(width, height)?;
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

#[cfg(test)]
mod tests {
    use super::*;
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
