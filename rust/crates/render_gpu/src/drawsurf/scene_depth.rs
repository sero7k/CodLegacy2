use bevy::camera::{Camera3d, Camera3dDepthLoadOp};
use bevy::prelude::*;
use bevy::camera::NormalizedRenderTarget;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_resource::*;
use bevy::render::renderer::RenderDevice;
use bevy::render::texture::TextureCache;
use bevy::render::view::{Msaa, ViewDepthTexture, ViewTarget};

pub const SCENE_DEPTH_FORMAT: TextureFormat = TextureFormat::Depth24PlusStencil8;

#[derive(Component)]
pub struct SceneDepthTexture {
    pub texture: Texture,
    attachment: ViewDepthTexture,
    sampled: TextureView,
}

impl SceneDepthTexture {
    /// The samples per pixel of this depth texture.
    pub fn sample_count(&self) -> u32 {
        self.texture.sample_count()
    }

    pub fn view(&self) -> &TextureView {
        &self.sampled
    }

    /// bo2zm: depth and stencil read-only, so the pass may also read the
    /// depth (`view`): Black Ops II's soft effect edges.
    pub fn read_only_attachment(&self) -> RenderPassDepthStencilAttachment<'_> {
        RenderPassDepthStencilAttachment {
            view: self.attachment.view(),
            depth_ops: None,
            stencil_ops: None,
        }
    }

    pub fn get_attachment(&self, store: StoreOp) -> RenderPassDepthStencilAttachment<'_> {
        let mut attachment = self.attachment.get_attachment(store);
        attachment.stencil_ops = Some(Operations {
            load: if attachment
                .depth_ops
                .as_ref()
                .is_some_and(|ops| matches!(ops.load, LoadOp::Clear(_)))
            {
                LoadOp::Clear(0)
            } else {
                LoadOp::Load
            },
            store: StoreOp::Store,
        });
        attachment
    }
}

pub(crate) fn prepare_scene_depth(
    mut commands: Commands,
    device: Res<RenderDevice>,
    mut cache: ResMut<TextureCache>,
    mut sampled_views: Local<std::collections::HashMap<TextureId, TextureView>>,
    mut logged: Local<std::collections::HashSet<String>>,
    views: Query<(
        Entity,
        &ExtractedCamera,
        &Camera3d,
        &Msaa,
        Option<&ViewTarget>,
    )>,
) {
    let mut textures = std::collections::HashMap::new();
    let mut live = Vec::new();
    for (entity, camera, settings, msaa, view_target) in &views {
        let Some(size) = camera.physical_target_size else {
            continue;
        };
        // The depth has the samples of the colour it is attached beside
        // (wgpu refuses a pass whose attachments differ): the view target's
        // own count, not the `Msaa` component read on its own.
        let colour_samples = colour_samples(view_target);
        let samples = colour_samples;
        let line = format!(
            "{} view: Msaa {:?}, colour {} sample(s), depth {} sample(s)",
            if matches!(camera.target, Some(NormalizedRenderTarget::Image(_))) {
                "image-target"
            } else {
                "window"
            },
            msaa,
            colour_samples,
            samples
        );
        if logged.insert(line.clone()) {
            info!("bo2mp scene depth: {line}");
        }
        let texture = textures
            .entry((camera.target.clone(), samples))
            .or_insert_with(|| {
                cache.get(
                    &device,
                    TextureDescriptor {
                        label: Some("scene_depth_stencil"),
                        size: Extent3d {
                            width: size.x,
                            height: size.y,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: TextureDimension::D2,
                        format: SCENE_DEPTH_FORMAT,
                        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                )
            })
            .clone();
        let id = texture.texture.id();
        live.push(id);
        let sampled = sampled_views
            .entry(id)
            .or_insert_with(|| {
                texture.texture.create_view(&TextureViewDescriptor {
                    aspect: TextureAspect::DepthOnly,
                    ..Default::default()
                })
            })
            .clone();
        let raw = texture.texture.clone();
        commands.entity(entity).insert(SceneDepthTexture {
            texture: raw,
            sampled,
            attachment: ViewDepthTexture::new(
                texture,
                match settings.depth_load_op {
                    Camera3dDepthLoadOp::Clear(value) => Some(value),
                    Camera3dDepthLoadOp::Load => None,
                },
            ),
        });
    }
    sampled_views.retain(|id, _| live.contains(id));
}

/// The samples per pixel of a view's colour: its sampled main texture's, or 1.
fn colour_samples(target: Option<&ViewTarget>) -> u32 {
    target
        .and_then(ViewTarget::sampled_main_texture)
        .map_or(1, |t| t.sample_count())
}

/// Whether a native renderer (it hands its frame over with the target's
/// plain, one-sample colour view) may draw into this view: the colour and
/// the scene depth must both be one sample. Says why once when not.
pub(crate) fn native_view_ok(who: &str, target: &ViewTarget, depth: &SceneDepthTexture) -> bool {
    let colour = colour_samples(Some(target));
    let ok = colour == 1 && depth.sample_count() == 1;
    if !ok {
        static SAID: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        if let Ok(mut said) = SAID.lock()
            && !said.iter().any(|w| w == who)
        {
            said.push(who.to_string());
            diag::warn!(
                World,
                "{who}: the view is multisampled (colour {colour}, depth {} sample(s)); the native frame needs one sample, so it is not drawn into this view",
                depth.sample_count()
            );
        }
    }
    ok
}

pub(crate) fn stencil_state(bits: u32) -> StencilState {
    if bits & 0x40 == 0 {
        return StencilState::default();
    }
    let op = |value: u32| match value & 7 {
        0 => StencilOperation::Keep,
        1 => StencilOperation::Zero,
        2 => StencilOperation::Replace,
        3 => StencilOperation::IncrementClamp,
        4 => StencilOperation::DecrementClamp,
        5 => StencilOperation::Invert,
        6 => StencilOperation::IncrementWrap,
        _ => StencilOperation::DecrementWrap,
    };
    let face = |shift: u32| {
        let value = bits >> shift;
        StencilFaceState {
            pass_op: op(value),
            fail_op: op(value >> 3),
            depth_fail_op: op(value >> 6),
            compare: match (value >> 9) & 7 {
                0 => CompareFunction::Never,
                1 => CompareFunction::Less,
                2 => CompareFunction::Equal,
                3 => CompareFunction::LessEqual,
                4 => CompareFunction::Greater,
                5 => CompareFunction::NotEqual,
                6 => CompareFunction::GreaterEqual,
                _ => CompareFunction::Always,
            },
        }
    };
    let front = face(8);
    StencilState {
        front,
        back: if bits & 0x80 != 0 { face(20) } else { front },
        read_mask: 0xff,
        write_mask: 0xff,
    }
}

/// The view of the scene depth texture for a native renderer's frame: kept
/// in `slot` while the texture is the same one (a new view every frame
/// otherwise; `BO2MP_NATIVE_VIEW_CACHE=0` makes it every frame again).
pub(super) fn cached_depth_view<'a>(slot: &'a mut Option<(TextureId, TextureView)>, texture: &Texture) -> &'a TextureView {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let cache = *CACHE.get_or_init(|| !std::env::var("BO2MP_NATIVE_VIEW_CACHE").is_ok_and(|v| v.trim() == "0"));
    if !cache || slot.as_ref().is_none_or(|(id, _)| *id != texture.id()) {
        *slot = Some((texture.id(), texture.create_view(&TextureViewDescriptor::default())));
    }
    &slot.as_ref().expect("set above").1
}
