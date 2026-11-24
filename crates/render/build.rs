use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let src = PathBuf::from("shaders").join("triangle.slang");
    println!("cargo:rerun-if-changed={}", src.display());
    compile(
        &src,
        "vertexMain",
        "vertex",
        &out_dir.join("triangle.vert.spv"),
    );
    compile(
        &src,
        "fragmentMain",
        "fragment",
        &out_dir.join("triangle.frag.spv"),
    );
}

fn compile(src: &Path, entry: &str, stage: &str, dst: &Path) {
    let output = Command::new(slangc())
        .arg(src)
        .args(["-entry", entry, "-stage", stage])
        .args(["-target", "spirv", "-profile", "spirv_1_6"])
        .arg("-fvk-use-entrypoint-name")
        .arg("-o")
        .arg(dst)
        .output()
        .unwrap_or_else(|err| panic!("slangc not found ({err}); install the Vulkan SDK or Slang"));
    if !output.status.success() {
        panic!(
            "slangc failed on {} ({entry}): {}",
            src.display(),
            String::from_utf8_lossy(&output.stderr)
        );
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
