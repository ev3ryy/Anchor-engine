use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum Optimize {
    Debug,
    Release,
}

pub fn compile_stage(
    src: &Path,
    entry: &str,
    stage: &str,
    include_dir: &Path,
    optimize: Optimize,
    dst: &Path,
) -> Result<(), String> {
    let output = Command::new(slangc())
        .arg(src)
        .args(["-entry", entry, "-stage", stage])
        .args(["-target", "spirv", "-profile", "spirv_1_6"])
        .args([
            "-fvk-use-entrypoint-name",
            "-emit-spirv-directly",
            "-fvk-use-scalar-layout",
        ])
        .arg(match optimize {
            Optimize::Debug => "-O0",
            Optimize::Release => "-O3",
        })
        .arg("-I")
        .arg(include_dir)
        .arg("-o")
        .arg(dst)
        .output()
        .map_err(|err| format!("slangc not found ({err}); install the Vulkan SDK or Slang"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn slangc() -> PathBuf {
    if let Ok(sdk) = env::var("VULKAN_SDK") {
        let windows = PathBuf::from(&sdk).join("Bin").join("slangc.exe");
        if windows.exists() {
            return windows;
        }
        let unix = PathBuf::from(&sdk).join("bin").join("slangc");
        if unix.exists() {
            return unix;
        }
    }
    PathBuf::from("slangc")
}
