use gpui_nightshade::{
    presentation::{AlphaMode, readback::ReadbackRing},
    wgpu,
};
#[test]
#[ignore = "requires a working native GPU adapter"]
fn asynchronous_ring_alignment_channels_backpressure_and_latest_frame() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    descriptor.backends = gpui_nightshade::default_backends();
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 65,
            height: 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut ring = ReadbackRing::new(
        &device,
        65,
        2,
        7,
        wgpu::TextureFormat::Bgra8Unorm,
        AlphaMode::Opaque,
        3,
    )
    .unwrap();
    for sequence in 0..3 {
        let bytes = [10, 20, 30 + sequence as u8, 0].repeat(65 * 2);
        queue.write_texture(
            texture.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(65 * 4),
                rows_per_image: Some(2),
            },
            texture.size(),
        );
        assert!(ring.enqueue(&device, &queue, &texture, sequence).unwrap());
    }
    assert!(!ring.enqueue(&device, &queue, &texture, 3).unwrap());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    // Wait for all callbacks on this test thread so latest-frame dropping is deterministic.
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let frame = loop {
        device.poll(wgpu::PollType::Poll).unwrap();
        if let Some(frame) = ring.take_latest() {
            break frame;
        }
        assert!(std::time::Instant::now() < deadline);
    };
    assert_eq!(frame.id.generation, 7);
    assert_eq!(frame.id.sequence, 2);
    assert_eq!(frame.rgba.len(), 65 * 2 * 4);
    assert!(
        frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [32, 20, 10, 255])
    );
    assert_eq!(ring.in_flight(), 0);
    assert_eq!(ring.dropped, 3);
    assert_eq!(ring.allocated_bytes(), 512 * 2 * 3);
}
