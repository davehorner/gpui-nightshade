use gpui_nightshade::{
    host::ViewportHost,
    presentation::{AlphaMode, readback::ReadbackRing},
    wgpu,
};
use nightshade_viewer_core::{ViewerArguments, ViewerConnection, ViewerCorePlugin, ViewerRuntime};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let width = std::env::var("VIEWER_TEST_WIDTH")
        .unwrap_or("640".into())
        .parse::<u32>()?;
    let height = std::env::var("VIEWER_TEST_HEIGHT")
        .unwrap_or("480".into())
        .parse::<u32>()?;
    if width == 0 || height == 0 {
        return Err("test dimensions must be positive".into());
    }
    let backends = match std::env::var("VIEWER_TEST_BACKEND").as_deref() {
        Ok("dx12") => wgpu::Backends::DX12,
        Ok("vulkan") => wgpu::Backends::VULKAN,
        Ok(_) => return Err("VIEWER_TEST_BACKEND must be dx12 or vulkan".into()),
        Err(_) => gpui_nightshade::default_backends(),
    };
    let connection = ViewerConnection::default();
    let mut runtime = pollster::block_on(ViewerRuntime::with_options(
        ViewerArguments {
            model: std::env::args().nth(1),
            no_agent: true,
            ..Default::default()
        },
        backends,
        width,
        height,
        connection.clone(),
        ViewerCorePlugin {
            persist_settings: false,
            remote_catalogs: false,
        },
    ))?;
    println!(
        "Adapter: {} {:?}, size {width}x{height}",
        runtime.renderer().gpu_profile.name,
        runtime.renderer().gpu_profile.backend
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        runtime.render()?;
        let snapshot = connection.snapshot().unwrap();
        if !snapshot.loading && !snapshot.nodes.is_empty() {
            println!(
                "Loaded {} nodes through trunk viewer schedules",
                snapshot.nodes.len()
            );
            break;
        }
        if std::time::Instant::now() > deadline {
            return Err("model loading timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    for _ in 0..std::env::var("VIEWER_TEST_FRAMES")
        .unwrap_or("4".into())
        .parse::<usize>()?
    {
        runtime.render()?;
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    assert!(
        runtime
            .world
            .res::<nightshade_platform::contract::window::Window>()
            .handle
            .is_none()
    );
    let renderer = runtime.renderer();
    let mut ring = ReadbackRing::new(
        &renderer.device,
        width,
        height,
        0,
        renderer.surface_format,
        AlphaMode::Opaque,
        3,
    )?;
    ring.enqueue(
        &renderer.device,
        &renderer.queue,
        renderer.external_output.as_ref().unwrap(),
        0,
    )?;
    loop {
        renderer.device.poll(wgpu::PollType::Poll)?;
        if let Some(frame) = ring.take_latest() {
            let colors: std::collections::HashSet<_> = frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2]])
                .collect();
            assert!(colors.len() > 8);
            println!(
                "Rendered glTF without a native Nightshade window: {} colors",
                colors.len()
            );
            break;
        }
        if std::time::Instant::now() > deadline {
            return Err("readback timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    Ok(())
}
