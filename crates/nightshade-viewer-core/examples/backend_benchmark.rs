//! Offscreen, single-frame-in-flight benchmark driven by the real MCP showcase.
use gpui_nightshade::{
    host::ViewportHost,
    presentation::{AlphaMode, readback::ReadbackRing},
    views::CameraLayout,
    wgpu,
};
use nightshade_viewer_core::{ViewerArguments, ViewerConnection, ViewerCorePlugin, ViewerRuntime};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 7 {
        return Err(
            "usage: backend_benchmark dx12|vulkan PORT WIDTH HEIGHT 1|4 CONTROL_FILE OUTPUT_PREFIX"
                .into(),
        );
    }
    let backend = match args[0].as_str() {
        "dx12" => wgpu::Backends::DX12,
        "vulkan" => wgpu::Backends::VULKAN,
        _ => return Err("backend must be dx12 or vulkan".into()),
    };
    let width = args[2].parse::<u32>()?;
    let height = args[3].parse::<u32>()?;
    if width == 0 || height == 0 {
        return Err("output dimensions must be positive".into());
    }
    let layout = match args[4].as_str() {
        "1" => CameraLayout::Single,
        "4" => CameraLayout::Quad,
        _ => return Err("view count must be 1 or 4".into()),
    };
    let control = PathBuf::from(&args[5]);
    let prefix = PathBuf::from(&args[6]);
    let mut runtime = pollster::block_on(ViewerRuntime::with_options(
        ViewerArguments {
            preview: true,
            agent_port: Some(args[1].parse()?),
            ..Default::default()
        },
        backend,
        width,
        height,
        ViewerConnection::default(),
        ViewerCorePlugin {
            persist_settings: false,
            remote_catalogs: false,
        },
    ))?;
    runtime.set_camera_layout(layout, 0)?;
    runtime
        .renderer()
        .device
        .poll(wgpu::PollType::wait_indefinitely())?;
    runtime.renderer_mut().timing.poll();
    let renderer = runtime.renderer();
    let timestamps = renderer.device.features().contains(
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
    );
    let metadata = serde_json::json!({
        "requested_backend":args[0],"actual_backend":format!("{:?}",renderer.gpu_profile.backend),
        "adapter":renderer.gpu_profile.name,"width":width,"height":height,"views":args[4],
        "optimized":!cfg!(debug_assertions),"gpu_timestamps":timestamps,
        "warmup_seconds_per_act":2,"frames_in_flight":1,
        "scope":"native offscreen render and synchronous completed readback; excludes GPUI upload/display, loading and client sleeps"
    });
    fs::write(
        prefix.with_extension("json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    let mut csv = std::io::BufWriter::new(fs::File::create(prefix.with_extension("csv"))?);
    writeln!(
        csv,
        "act,frame,phase_seconds,dispatch_ms,render_complete_ms,gpu_span_ms,readback_ms,total_ms,pixel_hash"
    )?;
    let mut ring = ReadbackRing::new(
        &renderer.device,
        width,
        height,
        0,
        renderer.surface_format,
        AlphaMode::Opaque,
        2,
    )?;
    let mut phase = String::new();
    let mut phase_start = Instant::now();
    let mut sequence = 0;
    let deadline = Instant::now() + Duration::from_secs(900);
    eprintln!("READY {} {}", args[0], renderer.gpu_profile.name);
    loop {
        if Instant::now() > deadline {
            return Err("benchmark control timeout".into());
        }
        let next = fs::read_to_string(&control).unwrap_or_else(|_| "idle".into());
        let next = next.trim();
        if next == "stop" {
            break;
        }
        if phase != next {
            phase = next.into();
            phase_start = Instant::now();
            eprintln!("PHASE {} {phase}", args[0]);
        }
        let started = Instant::now();
        runtime.render()?;
        let dispatch = started.elapsed().as_secs_f64() * 1000.;
        runtime
            .renderer()
            .device
            .poll(wgpu::PollType::wait_indefinitely())?;
        runtime.renderer_mut().timing.poll();
        let render_complete = started.elapsed().as_secs_f64() * 1000.;
        let gpu_span = runtime.renderer().timing.milliseconds;
        let copy_started = Instant::now();
        let renderer = runtime.renderer();
        ring.enqueue(
            &renderer.device,
            &renderer.queue,
            renderer
                .external_output
                .as_ref()
                .ok_or("no output texture")?,
            sequence,
        )?;
        renderer.device.poll(wgpu::PollType::wait_indefinitely())?;
        let pixels = ring.take_latest().ok_or("readback did not complete")?;
        let readback = copy_started.elapsed().as_secs_f64() * 1000.;
        let total = started.elapsed().as_secs_f64() * 1000.;
        if !phase.is_empty() && phase != "idle" && phase_start.elapsed() >= Duration::from_secs(2) {
            let hash = pixels
                .rgba
                .iter()
                .step_by(257)
                .fold(14695981039346656037u64, |h, b| {
                    (h ^ u64::from(*b)).wrapping_mul(1099511628211)
                });
            writeln!(
                csv,
                "{phase},{sequence},{:.6},{dispatch:.6},{render_complete:.6},{gpu_span:.6},{readback:.6},{total:.6},{hash}",
                phase_start.elapsed().as_secs_f64()
            )?;
        }
        sequence += 1;
    }
    csv.flush()?;
    eprintln!("DONE {} frames={sequence}", args[0]);
    Ok(())
}
