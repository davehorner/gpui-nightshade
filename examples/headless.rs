use gpui_nightshade::{
    host::SceneHost,
    presentation::{AlphaMode, readback::ReadbackRing},
    wgpu,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut host = pollster::block_on(SceneHost::cube(640, 480))?;
    println!(
        "Surface-free renderer: {} {:?}",
        host.renderer.gpu_profile.name, host.renderer.gpu_profile.backend
    );
    assert!(host.renderer.surface.is_none());
    host.renderer.set_owned_output(1280, 720)?;
    let mut ring = ReadbackRing::new(
        &host.renderer.device,
        1280,
        720,
        1,
        host.renderer.surface_format,
        AlphaMode::Opaque,
        3,
    )?;
    host.render()?;
    host.pick_request = Some((640, 360));
    host.render()?;
    ring.enqueue(
        &host.renderer.device,
        &host.renderer.queue,
        host.renderer.external_output.as_ref().unwrap(),
        0,
    )?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        host.renderer.device.poll(wgpu::PollType::Poll)?;
        if let Some(frame) = ring.take_latest() {
            let colors: std::collections::HashSet<_> = frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2]])
                .collect();
            assert!(
                colors.len() > 8,
                "expected rendered geometry, found {} colors",
                colors.len()
            );
            println!(
                "Rendered {}x{} cube; {} colors; readback {:?}; staging {} bytes",
                frame.width,
                frame.height,
                colors.len(),
                frame.latency,
                ring.allocated_bytes()
            );
            break;
        }
        if std::time::Instant::now() > deadline {
            return Err("readback timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        host.renderer.device.poll(wgpu::PollType::Poll)?;
        if let Some(selected) = host.poll_pick() {
            assert_eq!(selected, Some(1));
            println!("GPU picking selected cube entity 1");
            break;
        }
        if std::time::Instant::now() > deadline {
            return Err("GPU picking timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    Ok(())
}
