//! Native sampling isolation: no GPUI atlas, window, or swapchain participates.
use super::*;
use gpui_nightshade::host::{CameraPose, SceneHost, ViewportHost};
use windows::{
    Win32::{
        Foundation::{HMODULE, LUID},
        Graphics::{
            Direct3D::{Fxc::D3DCompile, *},
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
            0,
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
    run_sampling(1, false);
}

#[test]
#[ignore = "requires D3D12/D3D11 sharing on a native Windows GPU"]
fn pipelined_shared_frames_sample_without_gpui_compositor() {
    run_sampling(3, false);
}

#[test]
#[ignore = "requires D3D12/D3D11 sharing on a native Windows GPU"]
fn full_viewer_frames_sample_without_gpui_compositor() {
    run_sampling(3, true);
}

fn run_sampling(depth: usize, full_scene: bool) {
    let (width, height) = (1351, 715);
    let mut host: Box<dyn ViewportHost> = if full_scene {
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
    let mut ring = SharedRing::new(&host.renderer().device, width, height).unwrap();
    unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).unwrap();
        let adapter: IDXGIAdapter = factory
            .EnumAdapterByLuid(LUID {
                LowPart: ring.luid as u32,
                HighPart: (ring.luid >> 32) as i32,
            })
            .unwrap();
        let mut device = None;
        let mut context = None;
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
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
        let mut vs = None;
        let mut ps = None;
        device
            .CreateVertexShader(
                &compile(source, s!("vs"), s!("vs_5_0")),
                None,
                Some(&mut vs),
            )
            .unwrap();
        device
            .CreatePixelShader(
                &compile(source, s!("ps"), s!("ps_5_0")),
                None,
                Some(&mut ps),
            )
            .unwrap();
        context.VSSetShader(vs.as_ref().unwrap(), None);
        context.PSSetShader(ps.as_ref().unwrap(), None);
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
        let mut output = None;
        device
            .CreateTexture2D(&desc, None, Some(&mut output))
            .unwrap();
        let output = output.unwrap();
        let mut rtv = None;
        device
            .CreateRenderTargetView(&output, None, Some(&mut rtv))
            .unwrap();
        context.OMSetRenderTargets(Some(&[rtv]), None);
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
        let gate = if depth == 3 && frames >= 3 {
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
        let retire = |(fence, value, _frame, _texture, _srv, _owner)| {
            super::tests::wait(&fence, value);
        };
        for index in 0..frames {
            if in_flight.len() == depth {
                retire(in_flight.pop_front().unwrap());
            }
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
            used_slots.insert(slot.texture_handle.raw().0 as usize);
            super::tests::wait(&slot.ready_fence, frame.ready_value);
            let (texture, owner) =
                gpui_windows::shared_texture::import(&device, &context, frame.clone()).unwrap();
            let mut srv = None;
            device
                .CreateShaderResourceView(&texture, None, Some(&mut srv))
                .unwrap();
            owner.before_draw().unwrap();
            context.VSSetShaderResources(0, Some(&[srv.clone()]));
            context.PSSetShaderResources(0, Some(&[srv.clone()]));
            context.Draw(4, 0);
            owner.after_draw().unwrap();
            in_flight.push_back((
                slot.consumed_fence.clone(),
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
                let pending = ring
                    .slots
                    .iter()
                    .all(|slot| !slot.available() && slot.consumed_fence.GetCompletedValue() == 0);
                gate.0.Signal(1).unwrap();
                assert!(
                    pending,
                    "all three consumer reads must remain pending behind the gate"
                );
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
            "standalone D3D11: full_scene={full_scene}, depth={depth}, {frames} samples, {} colors, {width}x{height}",
            colors.len()
        );
    }
}
