//! Textures and buffers on the GPU.

use fastfile_t7::texture::ImageLevels;
use render_h1::gpu::Gpu;
use wgpu::util::DeviceExt;

pub struct Tex {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

/// A 2-D image's levels uploaded as stored (its own DXGI format).
pub fn upload(gpu: &Gpu, label: &str, im: &ImageLevels) -> Result<Tex, String> {
    let format = render_h1::gpu::dxgi_format(im.format).ok_or_else(|| format!("{label}: DXGI {} not drawn", im.format))?;
    if format.is_compressed() && !gpu.bc {
        return Err(format!("{label}: the device has no BC texture support"));
    }
    let (bw, bh) = format.block_dimensions();
    let bsize = format.block_copy_size(None).ok_or("format size")?;
    let mut levels = Vec::new();
    let mut at = 0usize;
    for l in 0..im.levels.max(1) {
        let w = (im.width >> l).max(1);
        let h = (im.height >> l).max(1);
        let bx = w.div_ceil(bw);
        let by = h.div_ceil(bh);
        let n = (bx * by * bsize) as usize;
        if at + n > im.data.len() {
            break;
        }
        levels.push((w, h, bx * bsize, by, at));
        at += n;
    }
    if levels.is_empty() {
        return Err(format!("{label}: no level fits its {} bytes", im.data.len()));
    }
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: im.width.div_ceil(bw) * bw, height: im.height.div_ceil(bh) * bh, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (l, &(w, h, row, rows, off)) in levels.iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: l as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &im.data[off..off + (row * rows) as usize],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(rows) },
            wgpu::Extent3d { width: w.div_ceil(bw) * bw, height: h.div_ceil(bh) * bh, depth_or_array_layers: 1 },
        );
    }
    let view = texture.create_view(&Default::default());
    Ok(Tex { texture, view })
}

pub fn buffer(gpu: &Gpu, label: &str, data: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    // Empty buffers can't be bound: four zero bytes stand for none.
    let contents: &[u8] = if data.is_empty() { &[0u8; 16] } else { data };
    gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage })
}

pub fn target(gpu: &Gpu, label: &str, w: u32, h: u32, format: wgpu::TextureFormat, extra: wgpu::TextureUsages) -> Tex {
    let texture = crate::audit::texture(&gpu, &wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | extra,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    Tex { texture, view }
}

#[cfg(test)]
mod tests {
    /// A streamed picture's whole chain (`T7Map::image`) uploads as a mip
    /// chain: BC1 (DXGI 71) 64x16 down to 1x1 and RGBA8 (28) 8x4.
    #[test]
    fn whole_chains_upload_with_their_mips() {
        let Ok(gpu) = render_h1::gpu::Gpu::headless() else { return eprintln!("skipped: no device") };
        for (format, w, h, block) in [(71u32, 64u32, 16u32, Some(8usize)), (28, 8, 4, None)] {
            if block.is_some() && !gpu.bc {
                continue;
            }
            let levels = fastfile_t7::texture::full_levels(w, h);
            let size: usize = (0..levels).map(|l| fastfile_t7::texture::level_size(format, (w >> l).max(1), (h >> l).max(1)).unwrap()).sum();
            let im = fastfile_t7::texture::ImageLevels { format, width: w, height: h, levels, data: vec![0x55; size], part: Some(2) };
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let t = super::upload(&gpu, "chain", &im).expect("upload");
            gpu.queue.submit([]);
            assert!(pollster::block_on(scope.pop()).is_none(), "format {format}: validation");
            assert_eq!(t.texture.mip_level_count(), levels, "format {format}");
        }
    }
}
