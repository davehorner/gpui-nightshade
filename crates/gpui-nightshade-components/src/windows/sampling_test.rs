//! Native sampling isolation: no GPUI atlas, window, or swapchain participates.
use super::*;
use gpui_nightshade::host::SceneHost;
use windows::{
    Win32::{
        Foundation::{HMODULE, LUID},
        Graphics::{
            Direct3D::{Fxc::D3DCompile, *},
            Direct3D11::*,
            Dxgi::*,
        },
    },
    core::{PCSTR, s},
};

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
    let (width, height) = (1351, 715);
    let mut host = pollster::block_on(SceneHost::cube_with_backends(
        width,
        height,
        wgpu::Backends::DX12,
    ))
    .unwrap();
    let mut ring = SharedRing::new(&host.renderer.device, width, height).unwrap();
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
    Out o; o.position=float4(uv.x*2-1,1-uv.y*2,0,1); o.uv=uv; return o;
}
float4 ps(Out i):SV_Target { return image.SampleLevel(sampler0,i.uv,0); }
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
                    Filter: D3D11_FILTER_MIN_MAG_MIP_POINT,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MaxLOD: f32::MAX,
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
        for index in 0..frames {
            host.yaw = index as f32 * 0.01;
            host.render().unwrap();
            let frame = ring
                .copy_frame(
                    &host.renderer.device,
                    &host.renderer.queue,
                    host.renderer.external_output.as_ref().unwrap(),
                )
                .unwrap()
                .unwrap();
            let slot = ring
                .slots
                .iter()
                .find(|slot| slot.leased.ptr_eq(&Arc::downgrade(&frame)))
                .unwrap();
            super::tests::wait(&slot.ready_fence, frame.ready_value);
            let (texture, owner) =
                gpui_windows::shared_texture::import(&device, &context, frame.clone()).unwrap();
            let mut srv = None;
            device
                .CreateShaderResourceView(&texture, None, Some(&mut srv))
                .unwrap();
            owner.before_draw().unwrap();
            context.PSSetShaderResources(0, Some(&[srv]));
            context.Draw(4, 0);
            owner.after_draw().unwrap();
            super::tests::wait(
                &slot.consumed_fence,
                frame.last_read.load(Ordering::Acquire),
            );
            device.GetDeviceRemovedReason().unwrap();
            drop(owner);
            drop(frame);
            if (index + 1) % 30 == 0 {
                eprintln!("standalone D3D11 sampled {} frames", index + 1);
            }
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
            "standalone D3D11: {frames} samples, {} colors, {width}x{height}",
            colors.len()
        );
    }
}
