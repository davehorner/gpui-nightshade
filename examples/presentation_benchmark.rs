//! Measures rendering and asynchronous GPU readback, excluding GPUI atlas upload/display.
use gpui_nightshade::{
    host::SceneHost,
    presentation::{AlphaMode, readback::ReadbackRing},
    wgpu,
};
use std::time::{Duration, Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let samples: u64 = std::env::var("BENCH_FRAMES")
        .unwrap_or("30".into())
        .parse()?;
    if samples == 0 {
        return Err("BENCH_FRAMES must be positive".into());
    }
    let mut host = pollster::block_on(SceneHost::cube(640, 480))?;
    host.render()?;
    eprintln!(
        "adapter={}, backend={:?}, profile=debug",
        host.renderer.gpu_profile.name, host.renderer.gpu_profile.backend
    );
    println!(
        "width,height,frames,elapsed_ms,render_cpu_ms,readback_mean_ms,readback_fps,dropped,staging_bytes"
    );
    for (width, height) in [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)] {
        host.renderer.set_owned_output(width, height)?;
        let mut ring = ReadbackRing::new(
            &host.renderer.device,
            width,
            height,
            1,
            host.renderer.surface_format,
            AlphaMode::Opaque,
            3,
        )?;
        host.render()?;
        let start = Instant::now();
        let mut submitted = 0;
        let mut received = 0;
        let mut latency = 0.;
        let mut render_cpu = 0.;
        let deadline = start + Duration::from_secs(120);
        while submitted < samples || ring.in_flight() > 0 {
            if submitted < samples && ring.in_flight() < 3 {
                let before = Instant::now();
                host.render()?;
                render_cpu += before.elapsed().as_secs_f64();
                ring.enqueue(
                    &host.renderer.device,
                    &host.renderer.queue,
                    host.renderer.external_output.as_ref().unwrap(),
                    submitted,
                )?;
                submitted += 1;
            }
            host.renderer.device.poll(wgpu::PollType::Poll)?;
            if let Some(frame) = ring.take_latest() {
                received += 1;
                latency += frame.latency.as_secs_f64();
            }
            if Instant::now() > deadline {
                return Err("benchmark timed out".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let elapsed = start.elapsed().as_secs_f64();
        println!(
            "{width},{height},{received},{:.3},{:.3},{:.3},{:.3},{},{}",
            elapsed * 1000.,
            render_cpu / submitted as f64 * 1000.,
            latency / received as f64 * 1000.,
            received as f64 / elapsed,
            ring.dropped,
            ring.allocated_bytes()
        );
    }
    Ok(())
}
