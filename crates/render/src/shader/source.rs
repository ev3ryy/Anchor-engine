#[cfg(debug_assertions)]
use std::fs;
#[cfg(debug_assertions)]
use std::path::Path;

#[cfg(debug_assertions)]
use super::compile::{self, Optimize};
#[cfg(debug_assertions)]
use super::scan::{self, Listed, SourceFile};
#[cfg(debug_assertions)]
use super::words;
#[cfg(debug_assertions)]
use crate::Error;

pub struct Compiled {
    pub name: String,
    pub vertex_entry: String,
    pub fragment_entry: String,
    pub vertex: Vec<u32>,
    pub fragment: Vec<u32>,
}

#[cfg(debug_assertions)]
pub fn load_directory(
    dir: &Path,
    optimize: Optimize,
) -> Result<Vec<Result<Compiled, Error>>, Error> {
    let listed = scan::list_graphics(dir).map_err(|message| Error::ShaderInterface(message))?;
    let mut loaded = Vec::new();
    for item in listed {
        loaded.push(match item {
            Listed::Failed { name, message } => Err(Error::ShaderCompile {
                file: name,
                message,
            }),
            Listed::Program(source) => compile_source(&source, dir, optimize),
        });
    }
    Ok(loaded)
}

#[cfg(debug_assertions)]
fn compile_source(
    source: &SourceFile,
    include_dir: &Path,
    optimize: Optimize,
) -> Result<Compiled, Error> {
    let temp = std::env::temp_dir().join("engine-shaders");
    fs::create_dir_all(&temp).map_err(Error::Shader)?;
    let vertex_path = temp.join(format!("{}.vert.spv", source.name));
    let fragment_path = temp.join(format!("{}.frag.spv", source.name));
    compile_one(
        source,
        include_dir,
        optimize,
        "vertex",
        &source.vertex,
        &vertex_path,
    )?;
    compile_one(
        source,
        include_dir,
        optimize,
        "fragment",
        &source.fragment,
        &fragment_path,
    )?;
    Ok(Compiled {
        name: source.name.clone(),
        vertex_entry: source.vertex.clone(),
        fragment_entry: source.fragment.clone(),
        vertex: read_spirv(&vertex_path)?,
        fragment: read_spirv(&fragment_path)?,
    })
}

#[cfg(debug_assertions)]
fn compile_one(
    source: &SourceFile,
    include_dir: &Path,
    optimize: Optimize,
    stage: &str,
    entry: &str,
    dst: &Path,
) -> Result<(), Error> {
    compile::compile_stage(&source.path, entry, stage, include_dir, optimize, dst).map_err(
        |message| Error::ShaderCompile {
            file: source.name.clone(),
            message,
        },
    )
}

#[cfg(debug_assertions)]
fn read_spirv(path: &Path) -> Result<Vec<u32>, Error> {
    let bytes = fs::read(path).map_err(Error::Shader)?;
    words::spirv_words(&bytes)
        .map_err(|message| Error::ShaderInterface(format!("{}: {message}", path.display())))
}
