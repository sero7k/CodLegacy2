use std::sync::{Arc, Mutex};

use bevy::mesh::VertexBufferLayout;
use bevy::platform::collections::HashMap;
use bevy::prelude::Resource;
use bevy::render::render_resource::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutId, ColorTargetState, DepthStencilState,
    MultisampleState, PipelineCompilationOptions, PipelineLayoutDescriptor, PrimitiveState,
    RawFragmentState, RawRenderPipelineDescriptor, RawVertexBufferLayout, RawVertexState,
    RenderPipeline, ShaderModule, ShaderModuleDescriptor, ShaderSource,
};
use bevy::render::renderer::RenderDevice;
use bevy::tasks::{Task, futures_lite::future};

use super::sm3_wgsl::{PASS_VERTEX_ENTRY, ValidatedPassWgsl};
use render_material::PortId;

/// bo2mp (memory): what a shader module is made from. Ports that differ
/// only in their arguments often have the same WGSL (Terminal: 7082 ports,
/// 5699 sources), so with `share_on` a module, and every pipeline built the
/// same way from it, is made once per source, not once per port.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum ModuleId {
    Port(PortId),
    /// The WGSL's hash and length.
    Source(u64, usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct ModuleKey {
    id: ModuleId,
    cached_lighting: bool,
}

/// `BO2MP_EXACT_PIPELINE_SHARE=0`: one module and pipeline per port, as before.
fn share_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_EXACT_PIPELINE_SHARE").map_or(true, |v| v.trim() != "0"))
}

/// `BO2MP_EXACT_PIPELINE_EVICT=0`: a map change keeps every earlier map's
/// pipelines, as before.
fn evict_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_EXACT_PIPELINE_EVICT").map_or(true, |v| v.trim() != "0"))
}

/// Everything a pipeline is built from: two requests with the same one get
/// the same pipeline.
#[derive(PartialEq, Eq, Hash)]
struct BuildKey {
    module: ModuleKey,
    fragment_entry: String,
    vertex_buffers: Vec<VertexBufferLayout>,
    targets: Vec<Option<ColorTargetState>>,
    primitive: PrimitiveState,
    depth_stencil: DepthStencilState,
    multisample: MultisampleState,
    layouts: [BindGroupLayoutId; 2],
}

impl BuildKey {
    fn new(module: ModuleKey, plan: &ExactPipelinePlan) -> Self {
        Self {
            module,
            fragment_entry: plan.fragment_entry.clone(),
            vertex_buffers: plan.vertex_buffers.clone(),
            targets: plan.targets.clone(),
            primitive: plan.primitive,
            depth_stencil: plan.depth_stencil.clone(),
            multisample: plan.multisample,
            layouts: [plan.constants_layout.id(), plan.textures_layout.id()],
        }
    }
}

#[derive(Clone)]
pub(super) enum ExactModuleSource {
    Port(Arc<ValidatedPassWgsl>),
    CachedLighting(Arc<str>),
}

impl ExactModuleSource {
    fn wgsl(&self) -> &str {
        match self {
            Self::Port(module) => &module.source,
            Self::CachedLighting(source) => source,
        }
    }
}

pub(super) struct ExactPipelinePlan {
    pub(super) label: String,
    pub(super) fragment_entry: String,
    pub(super) vertex_buffers: Vec<VertexBufferLayout>,
    pub(super) targets: Vec<Option<ColorTargetState>>,
    pub(super) primitive: PrimitiveState,
    pub(super) depth_stencil: DepthStencilState,
    pub(super) multisample: MultisampleState,
    pub(super) constants_layout: BindGroupLayout,
    pub(super) textures_layout: BindGroupLayout,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub(super) struct ExactPipelineSlot(u32);

enum SlotState {
    Building,
    Ready(RenderPipeline),
    /// bo2mp: dropped on a map change (its ports are no longer loaded).
    Evicted,
}

struct PortBuild {
    module: ModuleKey,
    created: Option<Arc<ShaderModule>>,
    source_len: usize,
    pipelines: Vec<(ExactPipelineSlot, RenderPipeline)>,
}

#[derive(Resource, Default)]
pub(super) struct ExactPipelineRegistry {
    slots: Vec<SlotState>,
    by_key: HashMap<super::colour_submit::ExactColourPipelineKey, ExactPipelineSlot>,
    modules: HashMap<ModuleKey, Arc<ShaderModule>>,

    queued: HashMap<
        ModuleKey,
        (
            ExactModuleSource,
            PortId,
            Vec<(ExactPipelineSlot, ExactPipelinePlan)>,
        ),
    >,
    jobs: Vec<Task<PortBuild>>,

    layouts: Mutex<HashMap<BindGroupLayoutDescriptor, BindGroupLayout>>,

    discovered: Mutex<Vec<super::colour_submit::ExactColourPipelineKey>>,

    /// bo2mp: the driver's pipeline cache on disk, opened on the first flush.
    disk: Option<Option<Arc<super::exact_pipecache::DiskCache>>>,

    /// bo2mp: each slot's module (to drop modules no slot uses).
    slot_module: Vec<ModuleKey>,
    /// bo2mp: the slot built from each `BuildKey` (`share_on`).
    by_build: HashMap<BuildKey, ExactPipelineSlot>,
    /// bo2mp: each port's module id, hashed once.
    source_ids: HashMap<(PortId, bool), ModuleId>,
    /// bo2mp: requests answered by a pipeline another port built.
    shared_n: usize,
    /// bo2mp: WGSL bytes of the modules made, for the census.
    module_bytes: usize,
    /// bo2mp: the port set the last eviction ran for.
    evicted_for: Option<u64>,
}

impl ExactPipelineRegistry {
    pub(super) fn bind_group_layout(
        &self,
        device: &RenderDevice,
        descriptor: &BindGroupLayoutDescriptor,
    ) -> BindGroupLayout {
        self.layouts
            .lock()
            .expect("exact pipeline layout cache is never poisoned")
            .entry(descriptor.clone())
            .or_insert_with_key(|descriptor| {
                device.create_bind_group_layout(descriptor.label.as_ref(), &descriptor.entries)
            })
            .clone()
    }

    pub(super) fn discover(&self, key: super::colour_submit::ExactColourPipelineKey) {
        self.discovered
            .lock()
            .expect("exact pipeline discovery list is never poisoned")
            .push(key);
    }

    pub(super) fn take_discovered(&mut self) -> Vec<super::colour_submit::ExactColourPipelineKey> {
        std::mem::take(
            &mut *self
                .discovered
                .lock()
                .expect("exact pipeline discovery list is never poisoned"),
        )
    }

    pub(super) fn slot(
        &self,
        key: &super::colour_submit::ExactColourPipelineKey,
    ) -> Option<ExactPipelineSlot> {
        self.by_key.get(key).copied()
    }

    pub(super) fn ready(&self, slot: ExactPipelineSlot) -> Option<&RenderPipeline> {
        match self.slots.get(slot.0 as usize) {
            Some(SlotState::Ready(pipeline)) => Some(pipeline),
            _ => None,
        }
    }

    pub(super) fn is_ready(&self, slot: ExactPipelineSlot) -> bool {
        matches!(self.slots.get(slot.0 as usize), Some(SlotState::Ready(_)))
    }

    pub(super) fn request(
        &mut self,
        key: super::colour_submit::ExactColourPipelineKey,
        source: ExactModuleSource,
        plan: ExactPipelinePlan,
    ) -> ExactPipelineSlot {
        if let Some(slot) = self.by_key.get(&key).copied() {
            return slot;
        }
        let cached_lighting = matches!(source, ExactModuleSource::CachedLighting(_));
        let module = ModuleKey {
            id: self.module_id(key.port, cached_lighting, &source),
            cached_lighting,
        };
        let build = share_on().then(|| BuildKey::new(module, &plan));
        if let Some(build) = &build
            && let Some(&slot) = self.by_build.get(build)
        {
            self.by_key.insert(key, slot);
            self.shared_n += 1;
            return slot;
        }
        let slot = ExactPipelineSlot(
            u32::try_from(self.slots.len()).expect("exact pipeline slot count fits u32"),
        );
        self.slots.push(SlotState::Building);
        self.slot_module.push(module);
        self.by_key.insert(key, slot);
        if let Some(build) = build {
            self.by_build.insert(build, slot);
        }

        self.queued
            .entry(module)
            .or_insert_with(|| (source, key.port, Vec::new()))
            .2
            .push((slot, plan));
        slot
    }

    fn module_id(&mut self, port: PortId, cached_lighting: bool, source: &ExactModuleSource) -> ModuleId {
        if !share_on() {
            return ModuleId::Port(port);
        }
        *self.source_ids.entry((port, cached_lighting)).or_insert_with(|| {
            use std::hash::{Hash, Hasher};
            let text = source.wgsl();
            let mut h = std::hash::DefaultHasher::new();
            text.hash(&mut h);
            ModuleId::Source(h.finish(), text.len())
        })
    }

    /// bo2mp (memory): on a new port set (a map change), drops the
    /// pipelines and modules only ports no longer loaded use. Their keys go
    /// too, so a port that comes back builds again.
    pub(super) fn evict_unadmitted(&mut self, generation: u64, admitted: impl Fn(PortId) -> bool) {
        if !evict_on() || self.evicted_for == Some(generation) {
            return;
        }
        self.evicted_for = Some(generation);
        if self.by_key.is_empty() {
            return;
        }
        let keys_before = self.by_key.len();
        let mut keep = vec![false; self.slots.len()];
        self.by_key.retain(|key, slot| {
            let loaded = admitted(key.port);
            if loaded {
                keep[slot.0 as usize] = true;
            }
            loaded
        });
        let mut freed = 0;
        for (state, keep) in self.slots.iter_mut().zip(&keep) {
            if !keep && matches!(state, SlotState::Ready(_)) {
                *state = SlotState::Evicted;
                freed += 1;
            }
        }
        let slots = &self.slots;
        self.by_build
            .retain(|_, slot| !matches!(slots[slot.0 as usize], SlotState::Evicted));
        let used: std::collections::HashSet<ModuleKey> = slots
            .iter()
            .zip(&self.slot_module)
            .filter(|(state, _)| !matches!(state, SlotState::Evicted))
            .map(|(_, module)| *module)
            .collect();
        let modules_before = self.modules.len();
        self.modules.retain(|module, _| used.contains(module));
        self.source_ids.retain(|(port, _), _| admitted(*port));
        diag::info!(
            World,
            "exact pipelines: new port set dropped {freed} pipelines, {} modules (keys {keys_before} -> {})",
            modules_before - self.modules.len(),
            self.by_key.len()
        );
    }

    pub(super) fn shared_n(&self) -> usize {
        self.shared_n
    }

    pub(super) fn flush(&mut self, device: &RenderDevice) {
        if self.queued.is_empty() {
            return;
        }

        let pool = assets::load_pool();
        let disk = self
            .disk
            .get_or_insert_with(|| super::exact_pipecache::open(device))
            .clone();
        for (module, (source, port, plans)) in self.queued.drain() {
            let device = device.clone();
            let existing = self.modules.get(&module).cloned();
            let disk = disk.clone();
            self.jobs.push(pool.spawn(async move {
                build_port(&device, module, port, source, existing, plans, disk.as_ref().map(|d| &d.cache))
            }));
        }
    }

    pub(super) fn poll(&mut self) {
        let in_flight = self.jobs.len();
        let mut index = 0;
        while index < self.jobs.len() {
            let Some(build) = future::block_on(future::poll_once(&mut self.jobs[index])) else {
                index += 1;
                continue;
            };

            drop(self.jobs.swap_remove(index));
            if let Some(created) = build.created {
                self.modules.insert(build.module, created);
                self.module_bytes += build.source_len;
            }
            for (slot, pipeline) in build.pipelines {
                self.slots[slot.0 as usize] = SlotState::Ready(pipeline);
            }
        }
        // The last build in flight is done: keep what was compiled.
        if in_flight > 0 && self.jobs.is_empty() {
            diag::info!(
                World,
                "exact pipelines: built, slots={} modules held={} module source={} KiB shared requests={}",
                self.slots.len(),
                self.modules.len(),
                self.module_bytes >> 10,
                self.shared_n
            );
            if let Some(Some(disk)) = &self.disk {
                disk.save();
            }
        }
    }

    pub(super) fn module_n(&self) -> usize {
        self.modules.len()
    }

    pub(super) fn building_n(&self) -> usize {
        self.jobs.len()
    }
}

fn build_port(
    device: &RenderDevice,
    module: ModuleKey,
    port: PortId,
    source: ExactModuleSource,
    existing: Option<Arc<ShaderModule>>,
    plans: Vec<(ExactPipelineSlot, ExactPipelinePlan)>,
    cache: Option<&wgpu::PipelineCache>,
) -> PortBuild {
    // CODLEGACY_PIPELINE_LOG=1: each build that took long (when hitches line up
    // with first-time shader building).
    let started = std::time::Instant::now();
    let build = build_port_inner(device, module, port, source, existing, plans, cache);
    static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *LOG.get_or_init(|| codlegacy_env::var_os("CODLEGACY_PIPELINE_LOG").is_some()) {
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        if ms >= 50.0 {
            diag::info!(
                World,
                "exact pipeline build {:016x}/{}: {} pipelines, new module {}, {ms:.0} ms",
                port.vertex_program_hash,
                port.vertex_type,
                build.pipelines.len(),
                build.created.is_some()
            );
        }
    }
    build
}

fn build_port_inner(
    device: &RenderDevice,
    module: ModuleKey,
    port: PortId,
    source: ExactModuleSource,
    existing: Option<Arc<ShaderModule>>,
    plans: Vec<(ExactPipelineSlot, ExactPipelinePlan)>,
    cache: Option<&wgpu::PipelineCache>,
) -> PortBuild {
    let label = format!(
        "iw4_exact_colour/{:016x}/{}{}",
        port.vertex_program_hash,
        port.vertex_type,
        if module.cached_lighting {
            "/cached"
        } else {
            ""
        }
    );

    let source_len = source.wgsl().len();
    let (shader, created) = match existing {
        Some(shader) => (shader, None),
        None => {
            let shader = Arc::new(unsafe {
                device.create_shader_module(ShaderModuleDescriptor {
                    label: Some(&label),
                    source: ShaderSource::Wgsl(std::borrow::Cow::Borrowed(source.wgsl())),
                })
            });
            (shader.clone(), Some(shader))
        }
    };
    let pipelines = plans
        .into_iter()
        .map(|(slot, plan)| (slot, build_pipeline(device, &shader, &plan, cache)))
        .collect();
    PortBuild {
        module,
        created,
        source_len,
        pipelines,
    }
}

fn build_pipeline(
    device: &RenderDevice,
    shader: &ShaderModule,
    plan: &ExactPipelinePlan,
    cache: Option<&wgpu::PipelineCache>,
) -> RenderPipeline {
    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some(&plan.label),
        bind_group_layouts: &[Some(&plan.constants_layout), Some(&plan.textures_layout)],
        immediate_size: 0,
    });
    let buffers: Vec<RawVertexBufferLayout> = plan
        .vertex_buffers
        .iter()
        .map(|buffer| RawVertexBufferLayout {
            array_stride: buffer.array_stride,
            attributes: &buffer.attributes,
            step_mode: buffer.step_mode,
        })
        .collect();
    let compilation_options = PipelineCompilationOptions {
        constants: &[],
        zero_initialize_workgroup_memory: false,
    };
    device.create_render_pipeline(&RawRenderPipelineDescriptor {
        label: Some(&plan.label),
        layout: Some(&layout),
        vertex: RawVertexState {
            module: shader,
            entry_point: Some(PASS_VERTEX_ENTRY),
            buffers: &buffers,
            compilation_options: compilation_options.clone(),
        },
        fragment: Some(RawFragmentState {
            module: shader,
            entry_point: Some(&plan.fragment_entry),
            targets: &plan.targets,
            compilation_options,
        }),
        primitive: plan.primitive,
        depth_stencil: Some(plan.depth_stencil.clone()),
        multisample: plan.multisample,
        multiview_mask: None,
        cache,
    })
}
