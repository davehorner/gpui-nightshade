use nightshade::prelude::nalgebra_glm as glm;
use nightshade_render::{components::render_world::*, data::config::*};
use nightshade_wgpu::backend::{
    CameraFrameInputs, CameraProjectionParams, install_presentation_passes,
};
pub use nightshade_wgpu::backend::{
    SingleViewHost, ViewFrame, WgpuRenderer, compose_single_view, render_frame, restore_single_view,
};

/// Surface-free scene host. Construct and use on the render worker.
pub struct SceneHost {
    pub renderer: WgpuRenderer,
    pub scene: SingleViewHost,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: glm::Vec3,
    pub pick_request: Option<(u32, u32)>,
    started: std::time::Instant,
}
impl SceneHost {
    pub async fn cube(width: u32, height: u32) -> Result<Self, Box<dyn std::error::Error>> {
        Self::cube_with_backends(
            width,
            height,
            nightshade_wgpu::wgpu::InstanceDescriptor::new_without_display_handle_from_env()
                .backends,
        )
        .await
    }
    pub async fn cube_with_backends(
        width: u32,
        height: u32,
        backends: nightshade_wgpu::wgpu::Backends,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut scene = SingleViewHost {
            scene_world: nightshade::ecs::World::tracking(),
            state: RendererState::default(),
            mesh_cache: Default::default(),
            texture_cache: Default::default(),
            settings: RenderSettings::default(),
            vsync: true,
            debug_draw: Default::default(),
        };
        scene.settings.atmosphere = Atmosphere::None;
        scene.settings.show_sky = false;
        scene.settings.unlit_mode = true;
        scene.settings.bloom_enabled = false;
        scene.settings.anti_aliasing = AntiAliasing::Fxaa;
        scene.state.occlusion_culling_enabled = false;
        nightshade_render::commands::mesh_cache::mesh_cache_insert(
            &mut scene.mesh_cache,
            "Cube".into(),
            nightshade_render::data::procedural_meshes::create_cube_mesh(),
        );
        let material =
            scene
                .state
                .render_materials
                .add(nightshade_render::data::material::Material {
                    base_color: [0.18, 0.58, 0.95, 1.],
                    ..Default::default()
                });
        // Nightshade reserves entity ID zero as the outline/background sentinel.
        scene.scene_world.spawn((Transform::default(),));
        scene.scene_world.spawn((
            MeshName("Cube".into()),
            MeshKey(nightshade_render::data::generational_registry::registry_name_hash("Cube")),
            MorphWeights::default(),
            CullingMask::default(),
            MaterialId(material),
            Transform::default(),
            Visible(true),
            Layer(nightshade_render::data::render_layer::RenderLayer::WORLD),
        ));
        Self::from_single_view(scene, width, height, backends).await
    }
    /// Builds the standard geometry/postprocess/outline graph around an extracted scene.
    /// Scene-world entities use Nightshade's render_world components, not simulation components.
    pub async fn from_single_view(
        scene: SingleViewHost,
        width: u32,
        height: u32,
        backends: nightshade_wgpu::wgpu::Backends,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut renderer =
            WgpuRenderer::new_headless_with_backends(width, height, backends).await?;
        let mut app = nightshade::app::App::new();
        app.add_plugins((
            nightshade_wgpu_shading::ShadingPlugin,
            nightshade_wgpu_geometry::GeometryRenderPlugin,
            nightshade_wgpu_outline::OutlineRenderPlugin,
        ));
        let (_, _, mut hooks) = nightshade_wgpu::commands::into_parts(app);
        hooks.publish_shader_modules(&renderer.shader_composer);
        let format = renderer.surface_format;
        let (graph, build) = renderer.graph_and_pass_build();
        hooks.configure_render_graph(graph, build, format);
        install_presentation_passes(&mut renderer)?;
        let mut host = Self {
            renderer,
            scene,
            yaw: 0.6,
            pitch: 0.35,
            distance: 4.,
            target: glm::vec3(0., 0., 0.),
            pick_request: None,
            started: std::time::Instant::now(),
        };
        host.render()?;
        Ok(host)
    }
    pub fn poll_pick(&mut self) -> Option<Option<u32>> {
        let result = nightshade_wgpu::backend::poll_depth_pick(&mut self.renderer)?;
        let center = (result.sample_size * result.sample_size / 2) as usize;
        let hit = result
            .depth_values
            .get(center)
            .is_some_and(|depth| *depth > 0.0);
        let selected = hit.then(|| result.entity_id_values[center]);
        let outlined: Vec<_> = self
            .scene
            .scene_world
            .query_tag_type::<Outlined>()
            .collect();
        for entity in outlined {
            self.scene
                .scene_world
                .storage
                .remove_tag_type::<Outlined>(entity);
        }
        if let Some(id) = selected {
            let entity = self
                .scene
                .scene_world
                .query_ref::<&MeshName>()
                .iter()
                .find_map(|(entity, _)| (entity.id == id).then_some(entity));
            if let Some(entity) = entity {
                self.scene
                    .scene_world
                    .storage
                    .add_tag_type::<Outlined>(entity);
            }
        }
        Some(selected)
    }
    pub fn render(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let width = self.renderer.surface_config.width;
        let height = self.renderer.surface_config.height;
        let position = glm::vec3(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        ) * self.distance
            + self.target;
        let camera = CameraFrameInputs::perspective(
            nightshade::ecs::Entity {
                id: u32::MAX,
                generation: 0,
            },
            glm::look_at(&position, &self.target, &glm::vec3(0., 1., 0.)),
            position,
            CameraProjectionParams {
                z_near: 0.1,
                z_far: 1000.,
                y_fov_rad: std::f32::consts::FRAC_PI_4,
                aspect: width as f32 / height as f32,
                orthographic: None,
            },
            EffectiveShading::from_settings(&self.scene.settings, &Default::default()),
        );
        let mut inputs = compose_single_view(
            &mut self.scene,
            ViewFrame {
                camera,
                viewport: (width, height),
                uptime_milliseconds: self.started.elapsed().as_millis() as u64,
                delta_time: 1. / 60.,
            },
        );
        inputs.frame.pick_request = self.pick_request.take();
        let result = render_frame(
            &mut self.renderer,
            &mut [],
            &mut inputs,
            &mut Default::default(),
        );
        restore_single_view(&mut self.scene, inputs);
        result.map_err(Into::into)
    }
}

pub fn block_on_cube(width: u32, height: u32) -> Result<SceneHost, Box<dyn std::error::Error>> {
    pollster::block_on(SceneHost::cube(width, height))
}

pub fn block_on_cube_dx12(
    width: u32,
    height: u32,
) -> Result<SceneHost, Box<dyn std::error::Error>> {
    pollster::block_on(SceneHost::cube_with_backends(
        width,
        height,
        nightshade_wgpu::wgpu::Backends::DX12,
    ))
}

pub type Vector3 = glm::Vec3;

pub fn block_on_cube_with_backends(
    backends: nightshade_wgpu::wgpu::Backends,
    width: u32,
    height: u32,
) -> Result<SceneHost, Box<dyn std::error::Error>> {
    pollster::block_on(SceneHost::cube_with_backends(width, height, backends))
}

/// Camera controls shared by simple scenes and full ECS-backed viewer hosts.
#[derive(Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: [f32; 3],
}

/// Worker-owned rendering lifecycle. A host is constructed on the render thread;
/// it need not be Send, and may retain Nightshade schedules and plugin guards.
pub trait ViewportHost {
    fn renderer(&self) -> &WgpuRenderer;
    fn renderer_mut(&mut self) -> &mut WgpuRenderer;
    fn render(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn set_camera(&mut self, pose: CameraPose);
    fn request_pick(&mut self, x: u32, y: u32);
    fn poll_pick(&mut self) -> Option<Option<u32>>;
    fn continuous(&self) -> bool {
        false
    }
    fn resize(&mut self, width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.renderer_mut().set_owned_output(width, height)
    }
}
impl ViewportHost for SceneHost {
    fn renderer(&self) -> &WgpuRenderer {
        &self.renderer
    }
    fn renderer_mut(&mut self) -> &mut WgpuRenderer {
        &mut self.renderer
    }
    fn render(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        SceneHost::render(self)
    }
    fn set_camera(&mut self, pose: CameraPose) {
        self.yaw = pose.yaw;
        self.pitch = pose.pitch;
        self.distance = pose.distance;
        self.target = glm::Vec3::from(pose.target);
    }
    fn request_pick(&mut self, x: u32, y: u32) {
        self.pick_request = Some((x, y));
    }
    fn poll_pick(&mut self) -> Option<Option<u32>> {
        SceneHost::poll_pick(self)
    }
}
/// Default factory for the independently usable cube viewport.
pub fn cube_viewport(
    backends: nightshade_wgpu::wgpu::Backends,
    width: u32,
    height: u32,
) -> Result<Box<dyn ViewportHost>, Box<dyn std::error::Error>> {
    Ok(Box::new(block_on_cube_with_backends(
        backends, width, height,
    )?))
}
