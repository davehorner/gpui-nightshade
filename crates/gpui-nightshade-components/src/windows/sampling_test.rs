//! Native sampling isolation without a GPUI atlas or window. An optional
//! unattached composition swapchain exercises flip-buffer/Present mechanics.
use super::*;
use gpui_nightshade::host::{CameraPose, SceneHost, ViewportHost};
use windows::{
    Win32::{
        Foundation::{HMODULE, LUID},
        Graphics::{
            Direct3D::{
                Fxc::{D3DCOMPILE_DEBUG, D3DCOMPILE_SKIP_OPTIMIZATION, D3DCompile},
                *,
            },
            Direct3D11::*,
            Dxgi::*,
        },
    },
    core::{Interface, PCSTR, s},
};

// Release even during unwinding: no failed assertion may strand a GPU wait.
struct QueueGate(ID3D12Fence);
impl Drop for QueueGate {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.Signal(1);
        }
    }
}

unsafe fn compile(source: &str, entry: PCSTR, target: PCSTR) -> Vec<u8> {
    let mut code = None;
    let mut errors = None;
    unsafe {
        let result = D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None,
            entry,
            target,
            if std::env::var_os("SHARED_SAMPLING_UNOPTIMIZED").is_some() {
                D3DCOMPILE_DEBUG | D3DCOMPILE_SKIP_OPTIMIZATION
            } else {
                0
            },
            0,
            &mut code,
            Some(&mut errors),
        );
        if let Err(error) = result {
            let detail = errors.map(|blob| {
                String::from_utf8_lossy(std::slice::from_raw_parts(
                    blob.GetBufferPointer().cast(),
                    blob.GetBufferSize().min(4096),
                ))
                .into_owned()
            });
            panic!("shader compilation failed: {error}: {detail:?}");
        }
        let code = code.unwrap();
        std::slice::from_raw_parts(code.GetBufferPointer().cast(), code.GetBufferSize()).to_vec()
    }
}

#[test]
#[ignore = "requires D3D12/D3D11 sharing on a native Windows GPU"]
fn changing_shared_frames_sample_without_gpui_compositor() {
    run_sampling(1, false, false, false, false);
}

#[test]
#[ignore = "requires D3D12/D3D11 sharing on a native Windows GPU"]
fn pipelined_shared_frames_sample_without_gpui_compositor() {
    run_sampling(3, false, false, false, false);
}

#[test]
#[ignore = "requires D3D12/D3D11 sharing on a native Windows GPU"]
fn full_viewer_frames_sample_without_gpui_compositor() {
    run_sampling(3, true, false, false, false);
}

#[test]
#[ignore = "requires D3D12/D3D11 sharing on a native Windows GPU"]
fn gpui_sprite_shader_samples_full_viewer_without_swapchain() {
    run_sampling(3, true, true, false, false);
}

#[test]
#[ignore = "requires D3D12/D3D11 sharing on a native Windows GPU"]
fn gpui_sprite_shader_samples_full_viewer_with_unattached_swapchain() {
    run_sampling(3, true, true, true, false);
}

#[test]
#[ignore = "requires D3D12/D3D11 sharing on a native Windows GPU"]
fn render_worker_samples_full_viewer_on_consumer_thread() {
    run_sampling(3, true, true, true, true);
}

struct NativeWorker(crate::worker::Worker);
impl NativeWorker {
    fn next(&mut self, width: u32, height: u32) -> Arc<SharedTexture> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if let Some(frame) = self.0.take_native_test_frame()
                && frame.width == width
                && frame.height == height
            {
                return frame;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "render worker publication timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
impl Drop for NativeWorker {
    fn drop(&mut self) {
        if let Some(thread) = self.0.shutdown() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn make_host(full_scene: bool, width: u32, height: u32) -> Box<dyn ViewportHost> {
    let host: Box<dyn ViewportHost> = if full_scene {
        use nightshade_viewer_core::{
            ViewerArguments, ViewerConnection, ViewerCorePlugin, ViewerRuntime,
        };
        let connection = ViewerConnection::default();
        let mut runtime = pollster::block_on(ViewerRuntime::with_options(
            ViewerArguments {
                no_agent: true,
                ..Default::default()
            },
            wgpu::Backends::DX12,
            width,
            height,
            connection.clone(),
            ViewerCorePlugin {
                persist_settings: false,
                remote_catalogs: false,
            },
        ))
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            runtime.render().unwrap();
            let snapshot = connection.snapshot().unwrap();
            if !snapshot.loading && !snapshot.nodes.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "viewer model loading timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Box::new(runtime)
    } else {
        Box::new(
            pollster::block_on(SceneHost::cube_with_backends(
                width,
                height,
                wgpu::Backends::DX12,
            ))
            .unwrap(),
        )
    };
    host
}

unsafe fn immutable_buffer(device: &ID3D11Device, words: &[u32], structured: bool) -> ID3D11Buffer {
    let mut buffer = None;
    unsafe {
        device
            .CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: std::mem::size_of_val(words) as u32,
                    Usage: D3D11_USAGE_IMMUTABLE,
                    BindFlags: if structured {
                        D3D11_BIND_SHADER_RESOURCE.0
                    } else {
                        D3D11_BIND_CONSTANT_BUFFER.0
                    } as u32,
                    MiscFlags: if structured {
                        D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32
                    } else {
                        0
                    },
                    StructureByteStride: if structured { 96 } else { 0 },
                    ..Default::default()
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: words.as_ptr().cast(),
                    ..Default::default()
                }),
                Some(&mut buffer),
            )
            .unwrap();
    }
    buffer.unwrap()
}

fn run_sampling(
    depth: usize,
    full_scene: bool,
    gpui_shader: bool,
    swapchain: bool,
    threaded: bool,
) {
    let (width, height) = (1351, 715);
    let mut worker = threaded.then(|| {
        let worker = crate::worker::Worker::new(
            gpui_nightshade::presentation::BackendPreference::WindowsShared,
            Arc::new(move |_, w, h| Ok(make_host(true, w, h))),
        );
        worker.request(crate::worker::ViewRequest {
            width,
            height,
            yaw: 0.6,
            pitch: 0.35,
            distance: 4.,
            target: [0.; 3],
            pick: None,
        });
        NativeWorker(worker)
    });
    let mut host = (!threaded).then(|| make_host(full_scene, width, height));
    let mut ring = host
        .as_ref()
        .map(|host| SharedRing::new(&host.renderer().device, width, height).unwrap());
    let mut first = worker.as_mut().map(|worker| worker.next(width, height));
    let luid = first
        .as_ref()
        .map(|frame| frame.adapter_luid)
        .unwrap_or_else(|| ring.as_ref().unwrap().luid);
    unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).unwrap();
        let adapter: IDXGIAdapter = factory
            .EnumAdapterByLuid(LUID {
                LowPart: luid as u32,
                HighPart: (luid >> 32) as i32,
            })
            .unwrap();
        let mut device = None;
        let mut context = None;
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT
                | if std::env::var_os("SHARED_SAMPLING_D3D11_DEBUG").is_some() {
                    D3D11_CREATE_DEVICE_DEBUG
                } else {
                    D3D11_CREATE_DEVICE_FLAG(0)
                },
            Some(&[D3D_FEATURE_LEVEL_11_1]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .unwrap();
        let device = device.unwrap();
        let context = context.unwrap();
        let source = r#"
Texture2D<float4> image : register(t0);
SamplerState sampler0 : register(s0);
struct Out { float4 position:SV_POSITION; float2 uv:TEXCOORD0; };
Out vs(uint id:SV_VertexID) {
    float2 uv=float2(id&1,(id>>1)&1);
    float2 size; image.GetDimensions(size.x, size.y);
    Out o; o.position=float4(uv.x*2-1,1-uv.y*2,0,1); o.uv=(uv*float2(1351,715))/size; return o;
}
float4 ps(Out i):SV_Target { return image.Sample(sampler0,i.uv); }
"#;
        // Compile the checked-in upstream shader itself, inlining its sole
        // include so no custom include handler or copied shader body is needed.
        let upstream_source =
            include_str!("../../../../upstream/gpui-snapshot/gpui-windows/src/shaders.hlsl")
                .replace(
                    "#include \"alpha_correction.hlsl\"",
                    include_str!(
                        "../../../../upstream/gpui-snapshot/gpui-windows/src/alpha_correction.hlsl"
                    ),
                );
        let (source, vertex_entry, pixel_entry) = if gpui_shader {
            (
                upstream_source.as_str(),
                s!("polychrome_sprite_vertex"),
                s!("polychrome_sprite_fragment"),
            )
        } else {
            (source, s!("vs"), s!("ps"))
        };
        let mut vs = None;
        let mut ps = None;
        device
            .CreateVertexShader(
                &compile(source, vertex_entry, s!("vs_5_0")),
                None,
                Some(&mut vs),
            )
            .unwrap();
        device
            .CreatePixelShader(
                &compile(source, pixel_entry, s!("ps_5_0")),
                None,
                Some(&mut ps),
            )
            .unwrap();
        context.VSSetShader(vs.as_ref().unwrap(), None);
        context.PSSetShader(ps.as_ref().unwrap(), None);
        if gpui_shader {
            use gpui_kit::PolychromeSprite;
            assert_eq!(std::mem::size_of::<PolychromeSprite>(), 96);
            assert_eq!(std::mem::offset_of!(PolychromeSprite, tile), 8);
            assert_eq!(std::mem::offset_of!(PolychromeSprite, grayscale), 40);
            assert_eq!(std::mem::offset_of!(PolychromeSprite, opacity), 44);
            assert_eq!(std::mem::offset_of!(PolychromeSprite, bounds), 48);
            assert_eq!(std::mem::offset_of!(PolychromeSprite, content_mask), 64);
            assert_eq!(std::mem::offset_of!(PolychromeSprite, corner_radii), 80);
            let mut globals = [0u32; 12];
            globals[4] = (width as f32).to_bits();
            globals[5] = (height as f32).to_bits();
            let globals = immutable_buffer(&device, &globals, false);
            let batch = immutable_buffer(&device, &[1, 0, 0, 0], false);
            context.VSSetConstantBuffers(0, Some(&[Some(globals.clone()), Some(batch.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(globals), Some(batch)]));
            // HLSL PolychromeSprite layout: 96 bytes. Only instance 1 has
            // nonempty bounds, so visible output also verifies batch offset.
            let mut instances = [0u32; 72];
            let sprite = &mut instances[24..48];
            sprite[8] = width;
            sprite[9] = height;
            sprite[11] = 1f32.to_bits();
            for offset in [14, 18] {
                sprite[offset] = (width as f32).to_bits();
            }
            for offset in [15, 19] {
                sprite[offset] = (height as f32).to_bits();
            }
            let instances = immutable_buffer(&device, &instances, true);
            let mut view = None;
            device
                .CreateShaderResourceView(&instances, None, Some(&mut view))
                .unwrap();
            context.VSSetShaderResources(1, Some(&[view.clone()]));
            context.PSSetShaderResources(1, Some(&[view]));
        }
        context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        let mut sampler = None;
        device
            .CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
                    AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
                    AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                    MaxLOD: f32::MAX,
                    MaxAnisotropy: 1,
                    ComparisonFunc: D3D11_COMPARISON_ALWAYS,
                    ..Default::default()
                },
                Some(&mut sampler),
            )
            .unwrap();
        context.PSSetSamplers(0, Some(&[sampler]));
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..Default::default()
        };
        // This composition swapchain deliberately has no DirectComposition
        // visual or HWND. It tests flip-buffer/Present mechanics, not DWM display.
        let swapchain = swapchain.then(|| {
            factory
                .CreateSwapChainForComposition(
                    &device,
                    &DXGI_SWAP_CHAIN_DESC1 {
                        Width: width,
                        Height: height,
                        Format: desc.Format,
                        SampleDesc: desc.SampleDesc,
                        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                        BufferCount: 3,
                        Scaling: DXGI_SCALING_STRETCH,
                        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                        ..Default::default()
                    },
                    None,
                )
                .unwrap()
        });
        let output: ID3D11Texture2D = if let Some(swapchain) = &swapchain {
            swapchain.GetBuffer(0).unwrap()
        } else {
            let mut output = None;
            device
                .CreateTexture2D(&desc, None, Some(&mut output))
                .unwrap();
            output.unwrap()
        };
        let mut rtv = None;
        device
            .CreateRenderTargetView(&output, None, Some(&mut rtv))
            .unwrap();
        context.OMSetRenderTargets(Some(&[rtv.clone()]), None);
        context.RSSetViewports(Some(&[D3D11_VIEWPORT {
            Width: width as f32,
            Height: height as f32,
            MaxDepth: 1.,
            ..Default::default()
        }]));
        let frames = std::env::var("SHARED_SAMPLING_FRAMES")
            .ok()
            .map(|n| n.parse::<u32>().unwrap())
            .unwrap_or(120);
        assert!(frames > 0);
        if swapchain.is_some() {
            assert!(frames >= 4, "swapchain test needs at least four samples");
        }
        let mut presents = 0u32;
        let gate = if !threaded && depth == 3 && frames >= 3 {
            let ring = ring.as_ref().unwrap();
            let fence: ID3D12Fence = ring.device.CreateFence(0, D3D12_FENCE_FLAG_SHARED).unwrap();
            let handle = SharedHandle::from_owned(
                ring.device
                    .CreateSharedHandle(&fence, None, 0x10000000, None)
                    .unwrap(),
            );
            let mut consumer_gate = None;
            device
                .cast::<ID3D11Device5>()
                .unwrap()
                .OpenSharedFence::<ID3D11Fence>(handle.raw(), &mut consumer_gate)
                .unwrap();
            context
                .cast::<ID3D11DeviceContext4>()
                .unwrap()
                .Wait(consumer_gate.as_ref().unwrap(), 1)
                .unwrap();
            Some(QueueGate(fence))
        } else {
            None
        };
        // Retain every imported object until its consumer fence retires. With
        // depth three, enqueue producer work while previous consumer reads may
        // still be executing; only the oldest lease applies backpressure.
        let mut in_flight = std::collections::VecDeque::new();
        let mut used_slots = std::collections::HashSet::new();
        let retire = |(fence, value, _frame, _texture, _srv, _owner): (
            ID3D11Fence,
            u64,
            Arc<SharedTexture>,
            ID3D11Texture2D,
            Option<ID3D11ShaderResourceView>,
            gpui_windows::shared_texture::ImportedTexture,
        )| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                let completed = fence.GetCompletedValue();
                assert_ne!(completed, u64::MAX, "consumer device lost");
                if completed >= value {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "consumer fence timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        for index in 0..frames {
            if in_flight.len() == depth {
                retire(in_flight.pop_front().unwrap());
            }
            let frame = if let Some(worker) = &mut worker {
                first.take().unwrap_or_else(|| worker.next(width, height))
            } else {
                let host = host.as_mut().unwrap();
                let ring = ring.as_mut().unwrap();
                if !full_scene {
                    host.set_camera(CameraPose {
                        yaw: index as f32 * 0.01,
                        pitch: 0.35,
                        distance: 4.,
                        target: [0.; 3],
                    });
                }
                host.render().unwrap();
                let frame = ring
                    .copy_frame(
                        &host.renderer().device,
                        &host.renderer().queue,
                        host.renderer().external_output.as_ref().unwrap(),
                    )
                    .unwrap()
                    .unwrap();
                let slot = ring
                    .slots
                    .iter()
                    .find(|slot| slot.leased.ptr_eq(&Arc::downgrade(&frame)))
                    .unwrap();
                super::tests::wait(&slot.ready_fence, frame.ready_value);
                frame
            };
            used_slots.insert(frame.texture_handle.raw().0 as usize);
            let mut consumed = None;
            device
                .cast::<ID3D11Device5>()
                .unwrap()
                .OpenSharedFence::<ID3D11Fence>(frame.consumed_fence_handle.raw(), &mut consumed)
                .unwrap();
            let (texture, owner) =
                gpui_windows::shared_texture::import(&device, &context, frame.clone()).unwrap();
            let mut srv = None;
            device
                .CreateShaderResourceView(&texture, None, Some(&mut srv))
                .unwrap();
            // Flip-model Present unbinds the backbuffer from the pipeline.
            context.OMSetRenderTargets(Some(&[rtv.clone()]), None);
            owner.before_draw().unwrap();
            context.VSSetShaderResources(0, Some(&[srv.clone()]));
            context.PSSetShaderResources(0, Some(&[srv.clone()]));
            context.DrawInstanced(4, 1, 0, 0);
            owner.after_draw().unwrap();
            in_flight.push_back((
                consumed.unwrap(),
                frame.last_read.load(Ordering::Acquire),
                frame,
                texture,
                srv,
                owner,
            ));
            if index == 2
                && let Some(gate) = &gate
            {
                // All three consumers are behind the gate. Their native frame
                // leases must keep all allocations unavailable to the producer.
                let pending =
                    ring.as_ref().unwrap().slots.iter().all(|slot| {
                        !slot.available() && slot.consumed_fence.GetCompletedValue() == 0
                    });
                gate.0.Signal(1).unwrap();
                assert!(
                    pending,
                    "all three consumer reads must remain pending behind the gate"
                );
            }
            // Drain the gated initial batch before Present: blocking Present
            // behind our own CPU-released gate would deadlock this test.
            if let Some(swapchain) = &swapchain
                && index >= 2
                && index + 1 < frames
            {
                swapchain.Present(0, DXGI_PRESENT(0)).ok().unwrap();
                presents += 1;
            }
            device.GetDeviceRemovedReason().unwrap();

            if (index + 1) % 30 == 0 {
                eprintln!("standalone D3D11 sampled {} frames", index + 1);
            }
        }
        while let Some(frame) = in_flight.pop_front() {
            retire(frame);
        }
        if frames >= 3 && depth == 3 {
            assert_eq!(used_slots.len(), 3, "pipeline must exercise all ring slots");
        }
        if swapchain.is_some() {
            assert_eq!(presents, frames - 3);
        }
        // Do not Present the final draw: read its known backbuffer before flip
        // rotation. Earlier draws exercised Present after releasing the gate.
        let mut staging = None;
        device
            .CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    ..desc
                },
                None,
                Some(&mut staging),
            )
            .unwrap();
        let staging = staging.unwrap();
        context.CopyResource(&staging, &output);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .unwrap();
        let mut colors = std::collections::HashSet::new();
        for row in 0..height as usize {
            let bytes = std::slice::from_raw_parts(
                mapped
                    .pData
                    .cast::<u8>()
                    .add(row * mapped.RowPitch as usize),
                width as usize * 4,
            );
            for pixel in bytes.as_chunks::<4>().0 {
                colors.insert([pixel[0], pixel[1], pixel[2]]);
            }
        }
        context.Unmap(&staging, 0);
        assert!(
            colors.len() > 8,
            "fullscreen shader must sample scene pixels; got {} colors",
            colors.len()
        );
        eprintln!(
            "standalone D3D11: threaded={threaded}, swapchain={}, presents={presents}, gpui_shader={gpui_shader}, full_scene={full_scene}, depth={depth}, {frames} samples, {} colors, {width}x{height}",
            swapchain.is_some(),
            colors.len()
        );
    }
}
