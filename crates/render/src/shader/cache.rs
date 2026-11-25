use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use ash::{Device, vk};

use crate::Error;

pub struct CacheFile {
    pub cache: vk::PipelineCache,
    pub path: Option<PathBuf>,
}

pub fn open(device: &Device, uuid: &[u8; 16]) -> Result<CacheFile, Error> {
    let path = resolve_path(uuid);
    if path.is_none() {
        eprintln!("pipeline cache will stay in memory");
    }
    if let Some(path) = &path
        && let Some(data) = read_initial(path)
    {
        match create(device, Some(&data)) {
            Ok(cache) => {
                return Ok(CacheFile {
                    cache,
                    path: Some(path.clone()),
                });
            }
            Err(_) => {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(CacheFile {
        cache: create(device, None)?,
        path,
    })
}

pub fn store(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("pipeline cache path has no file name"))?;
    let mut temporary_name = file_name.to_os_string();
    temporary_name.push(".tmp");
    let temporary = path.with_file_name(temporary_name);
    {
        let mut file = File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temporary, path)?;
    Ok(())
}

fn create(device: &Device, data: Option<&[u8]>) -> Result<vk::PipelineCache, Error> {
    let mut info = vk::PipelineCacheCreateInfo::default();
    if let Some(data) = data {
        info = info.initial_data(data);
    }
    Ok(unsafe { device.create_pipeline_cache(&info, None)? })
}

fn read_initial(path: &Path) -> Option<Vec<u8>> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() < 32 {
        let _ = fs::remove_file(path);
        return None;
    }
    Some(bytes)
}

fn resolve_path(uuid: &[u8; 16]) -> Option<PathBuf> {
    let name = uuid
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        + ".bin";
    if let Some(dir) = exe_dir()
        && writable(&dir)
    {
        return Some(dir.join(name));
    }
    if let Some(dir) = local_app_data_dir()
        && writable(&dir)
    {
        return Some(dir.join(name));
    }
    None
}

fn exe_dir() -> Option<PathBuf> {
    Some(
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("pipeline_cache"),
    )
}

fn local_app_data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("engine").join("pipeline_cache"))
}

fn writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write_probe");
    match File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}
