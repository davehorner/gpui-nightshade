//! Browser presentation: asynchronous WebGPU readback, no blocking device waits or OS threads.
use gpui_kit::RenderImage;
use gpui_nightshade::{
    host::CameraPose,
    presentation::{
        AlphaMode, BackendPreference, FrameInfo, PresentationBackend, PresentationStatus,
        readback::ReadbackRing,
    },
    wgpu,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};
#[wasm_bindgen::prelude::wasm_bindgen(
    inline_js = "export function reportNightshadeStage(stage,message) { window.dispatchEvent(new CustomEvent('nightshade-stage',{detail:{stage,message}})); } export function gpuWaitNow() { return performance.now(); }"
)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=reportNightshadeStage)]
    fn report_stage(stage: &str, message: &str);
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=gpuWaitNow)]
    fn gpu_wait_now() -> f64;
}
#[derive(Clone, Copy, PartialEq)]
pub struct ViewRequest {
    pub layout: crate::CameraLayout,
    pub active_view: usize,
    pub width: u32,
    pub height: u32,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: [f32; 3],
    pub zoom_anchor: Option<[f32; 3]>,
    pub pick: Option<(u32, u32, u64)>,
}
#[derive(Clone)]
pub(crate) struct ViewportFrame {
    pub image: Arc<RenderImage>,
    pub info: FrameInfo,
    pub benchmark: Option<crate::benchmark::Stamp>,
}
struct State {
    request: ViewRequest,
    input: gpui_nightshade::input::PointerQueue,
    status: PresentationStatus,
    image: Option<ViewportFrame>,
    changed: bool,
    quit: bool,
    diagnostics: String,
}
pub struct Worker {
    shared: Rc<RefCell<State>>,
}
impl Worker {
    pub fn new(preference: BackendPreference, factory: crate::SceneFactory) -> Self {
        let shared = Rc::new(RefCell::new(State {
            request: ViewRequest {
                layout: Default::default(),
                active_view: 0,
                width: 640,
                height: 480,
                yaw: 0.6,
                pitch: 0.35,
                distance: 4.,
                target: [0.; 3],
                zoom_anchor: None,
                pick: None,
            },
            input: Default::default(),
            status: PresentationStatus::initializing(preference),
            image: None,
            changed: false,
            quit: false,
            diagnostics: "Initializing Nightshade WebGPU".into(),
        }));
        let global = js_sys::global();
        let property =
            |object: &js_sys::Object, key: &str| js_sys::Reflect::get(object, &key.into()).ok();
        let navigator = property(&global, "navigator").map(js_sys::Object::from);
        let location = property(&global, "location").map(js_sys::Object::from);
        let ua = navigator
            .as_ref()
            .and_then(|n| property(n, "userAgent"))
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        let search = location
            .as_ref()
            .and_then(|l| property(l, "search"))
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        let options = crate::web_options::WebRenderOptions::for_browser(&ua, &search);
        let weak = Rc::downgrade(&shared);
        wasm_bindgen_futures::spawn_local(async move {
            let result = async {
                report_stage("starting","Creating the Nightshade WebGPU renderer");
                let mut host = factory(wgpu::Backends::BROWSER_WEBGPU, 640, 480).await?;
                report_stage("starting","Nightshade scene created; preparing the first frame");
                let lost=Arc::new(std::sync::Mutex::new(None));
                let report_lost=lost.clone();
                host.renderer().device.set_device_lost_callback(move |reason,message| {
                    *report_lost.lock().unwrap()=Some(format!("Nightshade WebGPU device lost ({reason:?}): {message}. GPU failure recorded; gpu-check.html can test copy/readback without the scene."));
                });
                let report_error=lost.clone();
                host.renderer().device.on_uncaptured_error(Arc::new(move |error| {
                    *report_error.lock().unwrap()=Some(format!("Nightshade WebGPU error: {error}"));
                }));
                report_stage("starting","Waiting for Nightshade startup rendering to complete on the GPU");
                wait_for_gpu(&host.renderer().queue,&lost,"Nightshade startup GPU work").await?;
                let mut announced=false;
                let mut size = (0, 0);
                let mut generation = 0;
                let mut sequence = 0;
                let mut ring: Option<ReadbackRing> = None;
                loop {
                    gloo_timers::future::TimeoutFuture::new(options.frame_ms).await;
                    let Some(shared) = weak.upgrade() else {
                        return Ok::<(), Box<dyn std::error::Error>>(());
                    };
                    if let Some(error)=lost.lock().unwrap().take(){return Err(error.into());}
                    let (request, visible, input) = {
                        let mut state = shared.borrow_mut();
                        if state.quit {
                            return Ok(());
                        }
                        let mut input = Vec::new();
                        while let Some(event) = state.input.pop() {
                            input.push(event);
                        }
                        (state.request, state.status.visible, input)
                    };
                    for event in input {
                        host.pointer_input(event);
                    }
                    if !visible {
                        host.service_hidden()?;
                        continue;
                    }
                    // Bound staging memory and browser raster work on high-DPI displays.
                    let next = options.render_size(request.width,request.height);
                    if next != size {
                        host.resize(next.0, next.1)?;
                        size = next;
                        generation += 1;
                        ring = Some(ReadbackRing::new(
                            &host.renderer().device,
                            size.0,
                            size.1,
                            generation,
                            host.renderer().surface_format,
                            AlphaMode::Opaque,
                            options.staging_frames,
                        )?);
                    }
                    host.set_camera_layout(request.layout, request.active_view)?;
                    host.set_camera_with_zoom_anchor(CameraPose {
                        yaw: request.yaw,
                        pitch: request.pitch,
                        distance: request.distance,
                        target: request.target,
                    }, request.zoom_anchor);
                    if let Some((x, y, _)) = request.pick {
                        host.request_pick(
                            ((u64::from(x)*u64::from(size.0)/u64::from(request.width.max(1))) as u32).min(size.0-1),
                            ((u64::from(y)*u64::from(size.1)/u64::from(request.height.max(1))) as u32).min(size.1-1),
                        );
                        shared.borrow_mut().request.pick = None;
                    }
                    let ring = ring.as_mut().expect("configured readback ring");
                    // Do not submit more GPU work while every readback is still in
                    // flight. Slow/mobile adapters must apply backpressure upstream.
                    if ring.in_flight()<options.staging_frames {
                        if sequence==0 {report_stage("starting","Rendering the first Nightshade frame");}
                        host.render()?;
                        if sequence==0 {
                            report_stage("starting","Waiting for the first Nightshade scene render to complete on the GPU");
                            wait_for_gpu(&host.renderer().queue,&lost,"First Nightshade frame GPU work").await?;
                            report_stage("starting","Copying the completed Nightshade frame for readback");
                        }
                        let renderer=host.renderer();
                        if let Some(texture) = renderer.external_output.as_ref() {
                            if ring.enqueue(&renderer.device, &renderer.queue, texture, sequence)? {sequence+=1;}
                        }
                    }
                    let renderer=host.renderer();
                    // WebGPU invokes map_async callbacks asynchronously when control returns to the browser.
                    if let Some(mut frame) = ring.take_latest() {
                        let info = frame.info();
                        if !announced {announced=true;report_stage("ready",&format!("First Nightshade frame: {}x{}",frame.width,frame.height));}
                        for pixel in frame.rgba.chunks_exact_mut(4) {
                            pixel.swap(0, 2);
                        }
                        let pixels =
                            image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
                                .ok_or("Invalid WebGPU frame")?;
                        let mut state = shared.borrow_mut();
                        state.image = Some(ViewportFrame {
                            image: Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])),
                            info,
                            benchmark: None,
                        });
                        state.status.active = Some(PresentationBackend::Readback);
                        state.status.graphics_backend = Some(wgpu::Backend::BrowserWebGpu);
                        state.status.adapter = Some(renderer.gpu_profile.name.clone());
                        state.changed = true;
                        state.diagnostics = format!(
                            "Nightshade WebGPU · {}×{} · frame {}",
                            size.0, size.1, info.id.sequence
                        );
                    }
                }
            }
            .await;
            if let Err(error) = result {
                report_stage("error", &error.to_string());
                if let Some(shared) = weak.upgrade() {
                    let mut state = shared.borrow_mut();
                    state.status.error = Some(error.to_string());
                    state.changed = true;
                }
            }
        });
        Self { shared }
    }
    pub fn shutdown(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.shared.borrow_mut().quit = true;
        None
    }
    pub fn request(&self, request: ViewRequest) {
        self.shared.borrow_mut().request = request;
    }
    pub fn camera_layout(&self) -> (crate::CameraLayout, usize) {
        let state = self.shared.borrow();
        (state.request.layout, state.request.active_view)
    }
    pub fn set_visible(&self, visible: bool) {
        self.shared.borrow_mut().status.visible = visible;
    }
    pub fn input(&self, event: gpui_nightshade::input::PointerInput) {
        self.shared.borrow_mut().input.push(event);
    }
    pub fn cancel_input(&self) {
        self.shared.borrow_mut().input.cancel();
    }
    pub fn presentation_status(&self) -> PresentationStatus {
        self.shared.borrow().status.clone()
    }
    pub fn has_update(&self) -> bool {
        self.shared.borrow().changed
    }
    pub fn paint_failed(&self, error: String) {
        let mut state = self.shared.borrow_mut();
        state.status.error = Some(error);
        state.changed = true;
    }
    pub fn take(&self) -> (Option<ViewportFrame>, String) {
        let mut state = self.shared.borrow_mut();
        state.changed = false;
        (
            state.image.take(),
            state
                .status
                .error
                .clone()
                .unwrap_or_else(|| state.diagnostics.clone()),
        )
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.borrow_mut().quit = true;
    }
}

// During startup, separate rendering completion from texture copy/map. Submission
// returning does not mean the GPU finished; a late loss must not blame readback.
async fn wait_for_gpu(queue: &wgpu::Queue,error: &Arc<std::sync::Mutex<Option<String>>>,label: &str) -> Result<(),Box<dyn std::error::Error>> {
    use std::sync::atomic::{AtomicBool,Ordering};
    let done=Arc::new(AtomicBool::new(false));let flag=done.clone();
    queue.on_submitted_work_done(move || {flag.store(true,Ordering::Release);});
    let started=gpu_wait_now();
    let mut next_report=15_000.0;
    loop {
        if let Some(message)=error.lock().unwrap().clone(){return Err(message.into());}
        let elapsed=gpu_wait_now()-started;
        if done.load(Ordering::Acquire){
            report_stage("starting",&format!("{label} completed in {:.1} seconds",elapsed/1000.0));
            return Ok(());
        }
        if elapsed>=60_000.0 {
            return Err(format!("{label} did not complete within 60 seconds; no first frame was presented").into());
        }
        if elapsed>=next_report {
            report_stage("starting",&format!("Waiting for {label}: {:.0} seconds elapsed (60 second startup limit)",elapsed/1000.0));
            next_report+=15_000.0;
        }
        gloo_timers::future::TimeoutFuture::new(10).await;
    }
}
