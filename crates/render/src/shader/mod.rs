mod cache;
#[cfg(any(debug_assertions, test))]
mod compile;
pub(crate) mod layout;
mod reflect;
#[cfg(any(debug_assertions, test))]
mod scan;
mod source;
pub(crate) mod state;
mod words;

use std::collections::HashMap;
use std::ffi::CString;
use std::path::PathBuf;

use ash::{Device, Instance, vk};

// Release still depends on `notify`. Cargo cannot gate that dependency on
// `debug_assertions`, and the watcher itself is debug-only.
#[cfg(not(debug_assertions))]
#[allow(unused_imports)]
use notify as _;

use crate::Error;
use crate::pipeline::{self, Modules};
use layout::{FRAME_PUSH_BYTES, Limits, frame_push_stages};
use source::Compiled;
use state::GraphicsState;

#[cfg(not(debug_assertions))]
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/shaders_gen.rs"));
}

#[cfg(debug_assertions)]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(debug_assertions)]
use std::time::{Duration, Instant};

pub struct DrawBinding {
    pub pipeline: vk::Pipeline,
    pub layout: vk::PipelineLayout,
    pub push_stages: vk::ShaderStageFlags,
    pub push_offset: u32,
    pub push_size: u32,
}

struct Gpu {
    modules: Modules,
    layout: vk::PipelineLayout,
    pipelines: HashMap<Vec<(u32, u32)>, vk::Pipeline>,
}

impl Gpu {
    fn into_retired(self) -> Retired {
        Retired {
            modules: self.modules,
            pipelines: self.pipelines.into_values().collect(),
        }
    }
}

struct Retired {
    modules: Modules,
    pipelines: Vec<vk::Pipeline>,
}

struct Program {
    vertex: Vec<u32>,
    fragment: Vec<u32>,
    vertex_entry: String,
    fragment_entry: String,
    state: GraphicsState,
    gpu: Gpu,
}

#[cfg(debug_assertions)]
struct Watch {
    _watcher: notify::RecommendedWatcher,
    dirty: Arc<AtomicBool>,
    since: Option<Instant>,
}

pub struct ShaderSet {
    device: Device,
    cache: vk::PipelineCache,
    cache_path: Option<PathBuf>,
    pipeline_layout: vk::PipelineLayout,
    programs: HashMap<String, Program>,
    retired: Vec<Retired>,
    limits: Limits,
    color_format: vk::Format,
    depth_format: vk::Format,
    #[cfg(debug_assertions)]
    watch: Option<Watch>,
}

impl Drop for ShaderSet {
    fn drop(&mut self) {
        self.save_cache();
        self.flush_retired();
        let programs = std::mem::take(&mut self.programs);
        for (_, program) in programs {
            self.destroy_retired(program.gpu.into_retired());
        }
        unsafe {
            self.device.destroy_pipeline_cache(self.cache, None);
        }
    }
}

impl ShaderSet {
    pub fn new(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &Device,
        color_format: vk::Format,
        depth_format: vk::Format,
        pipeline_layout: vk::PipelineLayout,
    ) -> Result<Self, Error> {
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let limits = Limits {
            max_push_constants_size: properties.limits.max_push_constants_size,
            max_bound_descriptor_sets: properties.limits.max_bound_descriptor_sets,
        };
        let cache = cache::open(device, &properties.pipeline_cache_uuid)?;
        let mut shaders = Self {
            device: device.clone(),
            cache: cache.cache,
            cache_path: cache.path,
            pipeline_layout,
            programs: HashMap::new(),
            retired: Vec::new(),
            limits,
            color_format,
            depth_format,
            #[cfg(debug_assertions)]
            watch: None,
        };
        shaders.load_initial()?;
        #[cfg(debug_assertions)]
        shaders.start_watch();
        Ok(shaders)
    }

    pub fn color_format(&self) -> vk::Format {
        self.color_format
    }

    pub fn depth_format(&self) -> vk::Format {
        self.depth_format
    }

    pub fn flush_retired(&mut self) {
        let retired = std::mem::take(&mut self.retired);
        for retired in retired {
            self.destroy_retired(retired);
        }
    }

    pub fn poll(&mut self) {
        #[cfg(debug_assertions)]
        self.poll_debug();
    }

    pub fn recreate(
        &mut self,
        color_format: vk::Format,
        depth_format: vk::Format,
    ) -> Result<(), Error> {
        if self.color_format == color_format && self.depth_format == depth_format {
            return Ok(());
        }
        self.color_format = color_format;
        self.depth_format = depth_format;
        let names: Vec<String> = self.programs.keys().cloned().collect();
        for name in names {
            self.rebuild(&name)?;
        }
        Ok(())
    }

    pub fn pipeline(&mut self, name: &str, spec: &[(u32, u32)]) -> Result<DrawBinding, Error> {
        let key = spec_key(spec)?;
        let ready = {
            let program = self
                .programs
                .get(name)
                .ok_or_else(|| Error::UnknownShader(name.to_string()))?;
            program.gpu.pipelines.contains_key(&key)
        };
        if !ready {
            self.create_variant(name, &key)?;
        }
        let program = self
            .programs
            .get(name)
            .ok_or_else(|| Error::UnknownShader(name.to_string()))?;
        let pipeline = *program
            .gpu
            .pipelines
            .get(&key)
            .ok_or_else(|| Error::ShaderInterface(format!("{name} is missing its pipeline")))?;
        Ok(DrawBinding {
            pipeline,
            layout: program.gpu.layout,
            push_stages: frame_push_stages(),
            push_offset: 0,
            push_size: FRAME_PUSH_BYTES,
        })
    }

    fn load_initial(&mut self) -> Result<(), Error> {
        let loaded = self.read_sources()?;
        if loaded.is_empty() {
            return Err(Error::ShaderInterface("no shaders found".into()));
        }
        for compiled in loaded {
            self.install(compiled?)?;
        }
        Ok(())
    }

    fn read_sources(&self) -> Result<Vec<Result<Compiled, Error>>, Error> {
        #[cfg(debug_assertions)]
        {
            source::load_directory(shaders_dir(), compile::Optimize::Debug)
        }
        #[cfg(not(debug_assertions))]
        {
            load_embedded()
        }
    }

    fn install(&mut self, compiled: Compiled) -> Result<(), Error> {
        let interface = reflect::reflect_graphics(
            &compiled.vertex,
            &compiled.vertex_entry,
            &compiled.fragment,
            &compiled.fragment_entry,
            &self.limits,
        )
        .map_err(Error::ShaderInterface)?;
        reflect::validate_bindless(&interface).map_err(Error::ShaderInterface)?;
        if !interface.vertex_inputs.is_empty() {
            return Err(Error::ShaderInterface(format!(
                "{} declares vertex inputs at locations {:?} without a vertex layout",
                compiled.name, interface.vertex_inputs
            )));
        }
        let state = state::description(&compiled.name);
        let gpu = self.build_gpu(
            &compiled.vertex,
            &compiled.fragment,
            &compiled.vertex_entry,
            &compiled.fragment_entry,
            state,
            &[Vec::new()],
        )?;
        let name = compiled.name.clone();
        let program = Program {
            vertex: compiled.vertex,
            fragment: compiled.fragment,
            vertex_entry: compiled.vertex_entry,
            fragment_entry: compiled.fragment_entry,
            state,
            gpu,
        };
        if let Some(old) = self.programs.insert(name, program) {
            self.retired.push(old.gpu.into_retired());
        }
        Ok(())
    }

    fn rebuild(&mut self, name: &str) -> Result<(), Error> {
        let (vertex, fragment, vertex_entry, fragment_entry, state, specs) = {
            let program = self
                .programs
                .get(name)
                .ok_or_else(|| Error::UnknownShader(name.to_string()))?;
            (
                program.vertex.clone(),
                program.fragment.clone(),
                program.vertex_entry.clone(),
                program.fragment_entry.clone(),
                program.state,
                program.gpu.pipelines.keys().cloned().collect::<Vec<_>>(),
            )
        };
        let gpu = self.build_gpu(
            &vertex,
            &fragment,
            &vertex_entry,
            &fragment_entry,
            state,
            &specs,
        )?;
        let program = self
            .programs
            .get_mut(name)
            .ok_or_else(|| Error::UnknownShader(name.to_string()))?;
        let old = std::mem::replace(&mut program.gpu, gpu);
        self.retired.push(old.into_retired());
        Ok(())
    }

    fn build_gpu(
        &mut self,
        vertex: &[u32],
        fragment: &[u32],
        vertex_entry: &str,
        fragment_entry: &str,
        state: GraphicsState,
        specs: &[Vec<(u32, u32)>],
    ) -> Result<Gpu, Error> {
        let modules = pipeline::create_modules(&self.device, vertex, fragment)?;
        let layout = self.pipeline_layout;
        let vertex_name = match CString::new(vertex_entry) {
            Ok(name) => name,
            Err(_) => {
                self.abandon(modules, Vec::new());
                return Err(Error::ShaderInterface(
                    "vertex entry name contains nul".into(),
                ));
            }
        };
        let fragment_name = match CString::new(fragment_entry) {
            Ok(name) => name,
            Err(_) => {
                self.abandon(modules, Vec::new());
                return Err(Error::ShaderInterface(
                    "fragment entry name contains nul".into(),
                ));
            }
        };
        let mut pipelines = HashMap::new();
        for spec in specs {
            match pipeline::create_graphics(
                &self.device,
                self.cache,
                &modules,
                &vertex_name,
                &fragment_name,
                layout,
                &state,
                self.color_format,
                self.depth_format,
                spec,
            ) {
                Ok(pipeline) => {
                    pipelines.insert(spec.clone(), pipeline);
                }
                Err(err) => {
                    self.abandon(modules, pipelines.into_values().collect());
                    return Err(err);
                }
            }
        }
        Ok(Gpu {
            modules,
            layout,
            pipelines,
        })
    }

    fn create_variant(&mut self, name: &str, spec: &[(u32, u32)]) -> Result<(), Error> {
        let (vertex_name, fragment_name, modules, layout, state) = {
            let program = self
                .programs
                .get(name)
                .ok_or_else(|| Error::UnknownShader(name.to_string()))?;
            (
                CString::new(program.vertex_entry.as_str())
                    .map_err(|_| Error::ShaderInterface("vertex entry name contains nul".into()))?,
                CString::new(program.fragment_entry.as_str()).map_err(|_| {
                    Error::ShaderInterface("fragment entry name contains nul".into())
                })?,
                Modules {
                    vertex: program.gpu.modules.vertex,
                    fragment: program.gpu.modules.fragment,
                },
                program.gpu.layout,
                program.state,
            )
        };
        let pipeline = pipeline::create_graphics(
            &self.device,
            self.cache,
            &modules,
            &vertex_name,
            &fragment_name,
            layout,
            &state,
            self.color_format,
            self.depth_format,
            spec,
        )?;
        self.programs
            .get_mut(name)
            .ok_or_else(|| Error::UnknownShader(name.to_string()))?
            .gpu
            .pipelines
            .insert(spec.to_vec(), pipeline);
        self.save_cache();
        Ok(())
    }

    fn abandon(&mut self, modules: Modules, pipelines: Vec<vk::Pipeline>) {
        self.destroy_retired(Retired { modules, pipelines });
    }

    fn destroy_retired(&mut self, retired: Retired) {
        unsafe {
            for pipeline in retired.pipelines {
                if pipeline != vk::Pipeline::null() {
                    self.device.destroy_pipeline(pipeline, None);
                }
            }
        }
        destroy_modules(&self.device, retired.modules);
    }

    fn save_cache(&self) {
        let Some(path) = &self.cache_path else {
            return;
        };
        let data = match unsafe { self.device.get_pipeline_cache_data(self.cache) } {
            Ok(data) => data,
            Err(err) => {
                eprintln!("failed to read pipeline cache: {err}");
                return;
            }
        };
        if let Err(err) = cache::store(path, &data) {
            eprintln!("failed to write pipeline cache: {err}");
        }
    }

    #[cfg(debug_assertions)]
    fn start_watch(&mut self) {
        let dirty = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&dirty);
        let mut watcher =
            match notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                if event.is_ok() {
                    flag.store(true, Ordering::Release);
                }
            }) {
                Ok(watcher) => watcher,
                Err(err) => {
                    eprintln!("shader watcher disabled: {err}");
                    return;
                }
            };
        if let Err(err) = notify::Watcher::watch(
            &mut watcher,
            shaders_dir(),
            notify::RecursiveMode::Recursive,
        ) {
            eprintln!("shader watcher disabled: {err}");
            return;
        }
        self.watch = Some(Watch {
            _watcher: watcher,
            dirty,
            since: None,
        });
    }

    #[cfg(debug_assertions)]
    fn poll_debug(&mut self) {
        let ready = {
            let Some(watch) = self.watch.as_mut() else {
                return;
            };
            if watch.dirty.swap(false, Ordering::AcqRel) {
                watch.since = Some(Instant::now());
            }
            match watch.since {
                Some(since) if since.elapsed() >= Duration::from_millis(100) => {
                    watch.since = None;
                    true
                }
                _ => false,
            }
        };
        if !ready {
            return;
        }
        self.reload();
        if self
            .watch
            .as_ref()
            .is_some_and(|watch| watch.dirty.swap(false, Ordering::AcqRel))
            && let Some(watch) = self.watch.as_mut()
        {
            watch.since = Some(Instant::now());
        }
    }

    #[cfg(debug_assertions)]
    fn reload(&mut self) {
        let loaded = match source::load_directory(shaders_dir(), compile::Optimize::Debug) {
            Ok(loaded) => loaded,
            Err(err) => {
                eprintln!("shader reload failed: {err}");
                return;
            }
        };
        let mut changed = false;
        for compiled in loaded {
            match compiled {
                Ok(compiled) => {
                    let name = compiled.name.clone();
                    if let Err(err) = self.install(compiled) {
                        eprintln!("shader {name}: {err}");
                    } else {
                        changed = true;
                    }
                }
                Err(err) => eprintln!("shader reload: {err}"),
            }
        }
        if changed {
            self.save_cache();
        }
    }
}

fn destroy_modules(device: &Device, modules: Modules) {
    pipeline::destroy_module(device, modules.vertex);
    pipeline::destroy_module(device, modules.fragment);
}

fn spec_key(spec: &[(u32, u32)]) -> Result<Vec<(u32, u32)>, Error> {
    let mut key = spec.to_vec();
    key.sort_by_key(|constant| constant.0);
    for pair in key.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(Error::ShaderInterface(format!(
                "duplicate specialization constant {}",
                pair[0].0
            )));
        }
    }
    Ok(key)
}

#[cfg(debug_assertions)]
fn shaders_dir() -> &'static std::path::Path {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders"))
}

#[cfg(not(debug_assertions))]
fn load_embedded() -> Result<Vec<Result<Compiled, Error>>, Error> {
    if embedded::PROGRAMS.is_empty() {
        return Err(Error::ShaderInterface("no shaders were embedded".into()));
    }
    let mut loaded = Vec::with_capacity(embedded::PROGRAMS.len());
    for program in embedded::PROGRAMS {
        loaded.push(embedded_program(program));
    }
    Ok(loaded)
}

#[cfg(not(debug_assertions))]
fn embedded_program(program: &embedded::EmbeddedProgram) -> Result<Compiled, Error> {
    Ok(Compiled {
        name: program.name.to_string(),
        vertex_entry: program.vertex_entry.to_string(),
        fragment_entry: program.fragment_entry.to_string(),
        vertex: words::spirv_words(program.vertex).map_err(|message| {
            Error::ShaderInterface(format!("{} vertex: {message}", program.name))
        })?,
        fragment: words::spirv_words(program.fragment).map_err(|message| {
            Error::ShaderInterface(format!("{} fragment: {message}", program.name))
        })?,
    })
}
