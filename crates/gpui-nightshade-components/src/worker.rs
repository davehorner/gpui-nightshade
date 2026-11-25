use gpui_kit::*;
use gpui_nightshade::presentation::BackendPreference;
use gpui_nightshade::{
    presentation::{AlphaMode, readback::ReadbackRing},
    wgpu,
};
use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

#[derive(Clone, Copy, PartialEq)]
pub struct ViewRequest {
    pub width: u32,
    pub height: u32,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: [f32; 3],
    pub pick: Option<(u32, u32, u64)>,
}

struct State {
    preference: BackendPreference,
    fallback: bool,
    request: ViewRequest,
    revision: u64,
    quit: bool,
    changed: bool,
    visible: bool,
    rendered_frames: u64,
    hidden_ticks: u64,
    image: Option<Arc<RenderImage>>,
    error: Option<String>,
    pub diagnostics: String,
    selected: Option<u32>,
    input: gpui_nightshade::input::PointerQueue,
}
pub struct Worker {
    shared: Arc<(Mutex<State>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new(preference: BackendPreference, factory: crate::SceneFactory) -> Self {
        let shared = Arc::new((
            Mutex::new(State {
                preference,
                fallback: false,
                selected: None,
                input: Default::default(),
                request: ViewRequest {
                    width: 640,
                    height: 480,
                    yaw: 0.6,
                    pitch: 0.35,
                    distance: 4.,
                    target: [0.; 3],
                    pick: None,
                },
                revision: 1,
                quit: false,
                changed: false,
                visible: true,
                rendered_frames: 0,
                hidden_ticks: 0,
                image: None,
                error: None,
                diagnostics: "Initializing GPU".into(),
            }),
            Condvar::new(),
        ));
        let thread_state = shared.clone();
        let thread = std::thread::Builder::new()
            .name("nightshade-render".into())
            .spawn(move || {
                if let Err(error) = run(&thread_state, &factory) {
                    let mut state = thread_state.0.lock().unwrap();
                    state.error = Some(error.to_string());
                    state.changed = true;
                }
            })
            .expect("start render worker");
        Self {
            shared,
            thread: Some(thread),
        }
    }
    pub fn shutdown(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.shared.0.lock().unwrap().quit = true;
        self.shared.1.notify_one();
        self.thread.take()
    }
    pub fn paint_failed(&self, error: String) {
        let mut state = self.shared.0.lock().unwrap();
        if state.preference == BackendPreference::Auto && !state.fallback {
            state.fallback = true;
            state.revision += 1;
            self.shared.1.notify_one();
        } else {
            state.error = Some(error);
            state.changed = true;
        }
    }
    pub fn request(&self, request: ViewRequest) {
        let mut state = self.shared.0.lock().unwrap();
        if state.request != request {
            state.request = request;
            state.revision += 1;
            self.shared.1.notify_one();
        }
    }
    pub fn set_visible(&self, visible: bool) {
        let mut state = self.shared.0.lock().unwrap();
        if state.visible != visible {
            state.visible = visible;
            state.input.cancel();
            state.request.pick = None;
            state.revision += 1;
            self.shared.1.notify_one();
        }
    }
    pub fn input(&self, event: gpui_nightshade::input::PointerInput) {
        let mut state = self.shared.0.lock().unwrap();
        state.input.push(event);
        self.shared.1.notify_one();
    }
    pub fn cancel_input(&self) {
        let mut state = self.shared.0.lock().unwrap();
        state.input.cancel();
        self.shared.1.notify_one();
    }
    pub fn has_update(&self) -> bool {
        self.shared.0.lock().unwrap().changed
    }
    pub fn take(&self) -> (Option<Arc<RenderImage>>, String) {
        let mut state = self.shared.0.lock().unwrap();
        state.changed = false;
        (
            state.image.take(),
            state.error.clone().unwrap_or_else(|| {
                format!("{} | selection {:?}", state.diagnostics, state.selected)
            }),
        )
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.0.lock().unwrap().quit = true;
        self.shared.1.notify_one();
    }
}
enum Transport {
    Readback(ReadbackRing),
    #[cfg(target_os = "windows")]
    Shared(crate::windows::SharedRing),
}
impl Transport {
    fn new(
        host: &dyn gpui_nightshade::host::ViewportHost,
        width: u32,
        height: u32,
        generation: u64,
        shared: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        #[cfg(target_os = "windows")]
        if shared {
            return Ok(Self::Shared(crate::windows::SharedRing::new(
                &host.renderer().device,
                width,
                height,
            )?));
        }
        let _ = shared;
        Ok(Self::Readback(ReadbackRing::new(
            &host.renderer().device,
            width,
            height,
            generation,
            host.renderer().surface_format,
            AlphaMode::Opaque,
            3,
        )?))
    }
    fn check_device(&self) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(target_os = "windows")]
        if let Self::Shared(ring) = self {
            ring.check_device()?;
        }
        Ok(())
    }
    fn can_resize(&self) -> bool {
        match self {
            Self::Readback(ring) => ring.in_flight() == 0,
            #[cfg(target_os = "windows")]
            Self::Shared(ring) => ring.producer_idle(),
        }
    }
    fn can_submit(&self) -> bool {
        match self {
            Self::Readback(ring) => ring.in_flight() < 3,
            #[cfg(target_os = "windows")]
            Self::Shared(ring) => ring.can_submit(),
        }
    }
    fn in_flight(&self) -> usize {
        match self {
            Self::Readback(ring) => ring.in_flight(),
            #[cfg(target_os = "windows")]
            Self::Shared(_) => 0,
        }
    }
}
fn run(
    shared: &Arc<(Mutex<State>, Condvar)>,
    factory: &crate::SceneFactory,
) -> Result<(), Box<dyn std::error::Error>> {
    let choice = shared.0.lock().unwrap().preference;
    if choice == BackendPreference::WindowsShared && !cfg!(target_os = "windows") {
        return Err("windows-shared is unavailable on this platform".into());
    }
    let mut gpu = cfg!(target_os = "windows") && choice != BackendPreference::Readback;
    let mut host = if gpu {
        match (factory)(wgpu::Backends::DX12, 640, 480) {
            Ok(host) => host,
            Err(error) if choice == BackendPreference::Auto => {
                eprintln!("DX12 initialization unavailable: {error}; selecting readback");
                gpu = false;
                (factory)(
                    wgpu::InstanceDescriptor::new_without_display_handle_from_env().backends,
                    640,
                    480,
                )?
            }
            Err(error) => return Err(error),
        }
    } else {
        (factory)(
            wgpu::InstanceDescriptor::new_without_display_handle_from_env().backends,
            640,
            480,
        )?
    };
    let mut generation = 0;
    let mut transport = match Transport::new(host.as_ref(), 640, 480, 0, gpu) {
        Ok(transport) => transport,
        Err(error) if choice == BackendPreference::Auto => {
            eprintln!("shared allocation unavailable: {error}; selecting readback");
            gpu = false;
            Transport::new(host.as_ref(), 640, 480, 0, false)?
        }
        Err(error) => return Err(error),
    };
    let mut applied = 0;
    let mut sequence = 0;
    let mut pick_sequence = None;
    let mut pick_pending = false;
    let mut last_camera = None;
    loop {
        transport.check_device()?;
        let (request, revision, fallback, input_pending, visible) = {
            let mut state = shared.0.lock().unwrap();
            while !state.quit
                && state.revision == applied
                && transport.in_flight() == 0
                && !pick_pending
                && !host.continuous()
                && state.input.is_empty()
            {
                state = shared.1.wait(state).unwrap();
            }
            if state.quit {
                return Ok(());
            }
            (
                state.request,
                state.revision,
                state.fallback,
                !state.input.is_empty(),
                state.visible,
            )
        };
        if !visible {
            if let Some(event) = shared.0.lock().unwrap().input.pop() {
                host.pointer_input(event);
            }
            host.service_hidden()?;
            // Finish outstanding copies without allocating/publishing new UI images.
            host.renderer().device.poll(wgpu::PollType::Poll)?;
            if let Transport::Readback(ring) = &mut transport {
                let _ = ring.take_latest();
            }
            pick_pending = false;
            applied = revision;
            let mut state = shared.0.lock().unwrap();
            state.hidden_ticks += 1;
            if !state.quit && !state.visible {
                let _ = shared
                    .1
                    .wait_timeout(state, Duration::from_millis(16))
                    .unwrap();
            }
            continue;
        }
        if gpu && fallback && transport.can_resize() {
            transport = Transport::new(
                host.as_ref(),
                host.renderer().surface_config.width,
                host.renderer().surface_config.height,
                generation,
                false,
            )?;
            gpu = false;
            applied = 0;
        }
        let resize_requested = (request.width, request.height)
            != (
                host.renderer().surface_config.width,
                host.renderer().surface_config.height,
            );
        if (revision != applied || host.continuous() || input_pending)
            && transport.can_submit()
            && (!resize_requested || transport.can_resize())
        {
            if request.width == 0 || request.height == 0 {
                applied = revision;
                continue;
            }
            if (request.width, request.height)
                != (
                    host.renderer().surface_config.width,
                    host.renderer().surface_config.height,
                )
            {
                generation += 1;
                // Validate replacement allocations before replacing the live generation.
                let replacement = Transport::new(
                    host.as_ref(),
                    request.width,
                    request.height,
                    generation,
                    gpu,
                )?;
                host.resize(request.width, request.height)?;
                transport = replacement;
            }
            let camera = gpui_nightshade::host::CameraPose {
                yaw: request.yaw,
                pitch: request.pitch,
                distance: request.distance,
                target: request.target,
            };
            if last_camera != Some(camera) {
                host.set_camera(camera);
                last_camera = Some(camera);
            }
            if let Some((x, y, sequence)) = request.pick
                && pick_sequence != Some(sequence)
            {
                host.request_pick(x, y);
                pick_sequence = Some(sequence);
                pick_pending = true;
            }
            if let Some(event) = shared.0.lock().unwrap().input.pop() {
                host.pointer_input(event);
            }
            host.render()?;
            shared.0.lock().unwrap().rendered_frames += 1;
            let source = host.renderer().external_output.as_ref().unwrap();
            match &mut transport {
                Transport::Readback(ring) => {
                    if ring.enqueue(
                        &host.renderer().device,
                        &host.renderer().queue,
                        source,
                        sequence,
                    )? {
                        applied = revision;
                        sequence += 1;
                    }
                }
                #[cfg(target_os = "windows")]
                Transport::Shared(ring) => {
                    if let Some(frame) =
                        ring.copy_frame(&host.renderer().device, &host.renderer().queue, source)?
                    {
                        applied = revision;
                        sequence += 1;
                        let mut state = shared.0.lock().unwrap();
                        state.image = Some(Arc::new(RenderImage::from_external(frame)));
                        state.changed = true;
                        state.diagnostics = format!(
                            "{} DX12 -> D3D11 shared texture | GPU copy | {}x{} | frame {}",
                            host.renderer().gpu_profile.name,
                            request.width,
                            request.height,
                            sequence
                        );
                    } else {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
            }
        }
        host.renderer().device.poll(wgpu::PollType::Poll)?;
        if let Some(entity) = host.poll_pick() {
            pick_pending = false;
            let mut state = shared.0.lock().unwrap();
            state.selected = entity;
            state.changed = true;
            state.revision += 1;
        }
        if let Transport::Readback(ring) = &mut transport
            && let Some(mut frame) = ring.take_latest()
        {
            for pixel in frame.rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
                .ok_or("invalid frame length")?;
            let image = Arc::new(RenderImage::new(vec![image::Frame::new(image)]));
            let mut state = shared.0.lock().unwrap();
            if frame.id.generation == generation {
                state.image = Some(image);
                state.changed = true;
                state.diagnostics = format!(
                    "{} {:?} | asynchronous readback | {}x{} | {:.1} ms | {} dropped | {} KiB staging",
                    host.renderer().gpu_profile.name,
                    host.renderer().gpu_profile.backend,
                    frame.width,
                    frame.height,
                    frame.latency.as_secs_f64() * 1000.,
                    ring.dropped,
                    ring.allocated_bytes() / 1024
                );
            }
        }
        if host.continuous() {
            std::thread::sleep(Duration::from_millis(16));
        } else if transport.in_flight() > 0 || revision != applied || pick_pending {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_nightshade::host::{CameraPose, SceneHost, ViewportHost, WgpuRenderer};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    struct ContinuousHost(SceneHost);
    impl ViewportHost for ContinuousHost {
        fn renderer(&self) -> &WgpuRenderer {
            &self.0.renderer
        }
        fn renderer_mut(&mut self) -> &mut WgpuRenderer {
            &mut self.0.renderer
        }
        fn render(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            self.0.render()
        }
        fn set_camera(&mut self, pose: CameraPose) {
            self.0.set_camera(pose);
        }
        fn request_pick(&mut self, x: u32, y: u32) {
            self.0.request_pick(x, y);
        }
        fn poll_pick(&mut self) -> Option<Option<u32>> {
            self.0.poll_pick()
        }
        fn continuous(&self) -> bool {
            true
        }
    }
    fn wait_for(worker: &Worker, predicate: impl Fn(&State) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            let state = worker.shared.0.lock().unwrap();
            assert!(state.error.is_none(), "{:?}", state.error);
            if predicate(&state) {
                break;
            }
            assert!(Instant::now() < deadline, "render worker timed out");
            drop(state);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    #[ignore = "requires a working native GPU adapter"]
    fn hidden_worker_services_without_presenting_and_resumes_same_host() {
        let constructions = Arc::new(AtomicUsize::new(0));
        let count = constructions.clone();
        let factory: crate::SceneFactory = Arc::new(move |backends, w, h| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(ContinuousHost(pollster::block_on(
                SceneHost::cube_with_backends(w, h, backends),
            )?)))
        });
        let mut worker = Worker::new(BackendPreference::Readback, factory);
        wait_for(&worker, |s| s.rendered_frames >= 2);
        worker.set_visible(false);
        wait_for(&worker, |s| s.hidden_ticks >= 2);
        let frames = worker.shared.0.lock().unwrap().rendered_frames;
        let mut request = worker.shared.0.lock().unwrap().request;
        request.width = 321;
        request.height = 187;
        worker.request(request);
        wait_for(&worker, |s| s.hidden_ticks >= 6);
        assert_eq!(worker.shared.0.lock().unwrap().rendered_frames, frames);
        worker.set_visible(true);
        wait_for(&worker, |s| s.rendered_frames > frames);
        assert_eq!(constructions.load(Ordering::SeqCst), 1);
        worker.shutdown().unwrap().join().unwrap();
    }
}
