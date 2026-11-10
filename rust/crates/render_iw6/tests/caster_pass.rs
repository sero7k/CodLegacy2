//! The sun shadow proxies' pass on a software device: a capsule drawn into a
//! depth atlas covers the texels under it and leaves the rest cleared.

use glam::{Mat4, Vec3};
use render_h1::gpu::Gpu;
use render_iw6::caster::{Casters, Caster};

#[test]
fn a_capsule_is_drawn_into_its_cascade_only() {
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no device");
        return;
    };
    const N: u32 = 64; // the atlas: 2 x 32
    let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test_atlas"),
        size: wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: render_iw6::world::SHADOW_DEPTH,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = depth.create_view(&Default::default());
    // Clear to 1.
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    drop(enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    let pass = Casters::new(&gpu.device);
    // Looking down x: world y to the right, z up, 100 units across, depth over 0..200.
    let vp = Mat4::orthographic_rh(-50.0, 50.0, -50.0, 50.0, 0.0, 200.0)
        * Mat4::look_at_rh(Vec3::new(-100.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z);
    let casters = [Caster { a: [0.0, 0.0, -20.0], b: [0.0, 0.0, 20.0], radius: 10.0 }];
    pass.upload(&gpu.queue, [0.0; 3], [vp, vp], &casters);
    // Cascade 0 only: the left half [0, 0, 32]... (x, y, size).
    pass.record(&mut enc, &view, 0, [0.0, 0.0, 32.0], casters.len());
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("read"),
        size: u64::from(N * 256),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    // (Row pitch 256 bytes = 64 f32.)
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &depth, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::DepthOnly },
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(N) } },
        wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = slice.get_mapped_range();
    let at = |x: usize, y: usize| f32::from_le_bytes(data[y * 256 + x * 4..y * 256 + x * 4 + 4].try_into().unwrap());
    // The capsule's middle (cascade 0's centre) is nearer than the clear.
    assert!(at(16, 16) < 0.9, "centre {}", at(16, 16));
    // Cascade 0's corner is empty; cascade 1's quarter (x 32..) was not drawn.
    assert_eq!(at(1, 1), 1.0);
    assert_eq!(at(48, 16), 1.0);
}
