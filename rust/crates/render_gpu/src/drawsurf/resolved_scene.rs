use bevy::prelude::*;
use bevy::render::render_resource::{
    CommandEncoder, Texture, TextureDescriptor, TextureDimension, TextureUsages, TextureView,
    TextureViewDescriptor,
};
use bevy::render::renderer::RenderDevice;

#[derive(Resource, Default)]
pub(super) struct ResolvedScene {
    target: Option<Texture>,
    view: Option<TextureView>,
}

impl ResolvedScene {
    pub fn ensure(&mut self, device: &RenderDevice, source: &Texture) -> (TextureView, bool) {
        let format = source.format().remove_srgb_suffix();
        let changed = self
            .target
            .as_ref()
            .is_none_or(|t| t.size() != source.size() || t.format() != format);
        if changed {
            let texture = device.create_texture(&TextureDescriptor {
                label: Some("exact_resolved_post_sun"),
                size: source.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.view = Some(texture.create_view(&TextureViewDescriptor::default()));
            self.target = Some(texture);
        }
        (
            self.view.as_ref().expect("ensure creates the view").clone(),
            changed,
        )
    }

    /// As `ensure`, for a source of `size` and `format` that is not a
    /// texture of Bevy's (Ghosts' HDR scene).
    pub fn ensure_like(
        &mut self,
        device: &RenderDevice,
        size: bevy::render::render_resource::Extent3d,
        format: bevy::render::render_resource::TextureFormat,
    ) -> (TextureView, bool) {
        let changed = self
            .target
            .as_ref()
            .is_none_or(|t| t.size() != size || t.format() != format);
        if changed {
            let texture = device.create_texture(&TextureDescriptor {
                label: Some("exact_resolved_post_sun"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.view = Some(texture.create_view(&TextureViewDescriptor::default()));
            self.target = Some(texture);
        }
        (
            self.view.as_ref().expect("ensure_like creates the view").clone(),
            changed,
        )
    }

    /// Copies a raw wgpu texture of the target's format (Ghosts' fogged
    /// scene); false when the format or size does not match.
    pub fn copy_raw(&self, encoder: &mut CommandEncoder, source: &wgpu::Texture) -> bool {
        let Some(target) = self.target.as_ref() else {
            return false;
        };
        if source.format() != target.format() || source.size() != target.size() {
            return false;
        }
        encoder.copy_texture_to_texture(source.as_image_copy(), target.as_image_copy(), source.size());
        true
    }

    pub fn copy(&self, encoder: &mut CommandEncoder, source: &Texture) {
        let target = self.target.as_ref().expect("ensure before recording");
        encoder.copy_texture_to_texture(
            source.as_image_copy(),
            target.as_image_copy(),
            source.size(),
        );
    }
}
