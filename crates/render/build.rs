use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

#[path = "src/shader/compile.rs"]
mod compile;
#[path = "src/shader/scan.rs"]
mod scan;

fn main() {
    let profile = env::var("PROFILE").unwrap_or_default();
    if profile != "release" {
        return;
    }

    println!("cargo:rerun-if-changed=shaders");
    println!("cargo:rerun-if-changed=src/shader/scan.rs");
    println!("cargo:rerun-if-changed=src/shader/compile.rs");

    let out_dir = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).to_path_buf();
    let shaders = Path::new("shaders");
    let listed = scan::list_graphics(shaders)
        .unwrap_or_else(|message| panic!("failed to list shaders: {message}"));

    let mut manifest = String::from(
        "pub struct EmbeddedProgram {\n    \
         pub name: &'static str,\n    \
         pub vertex_entry: &'static str,\n    \
         pub fragment_entry: &'static str,\n    \
         pub vertex: &'static [u8],\n    \
         pub fragment: &'static [u8],\n\
         }\n\n\
         pub static PROGRAMS: &[EmbeddedProgram] = &[\n",
    );

    for item in listed {
        let scan::Listed::Program(source) = item else {
            let scan::Listed::Failed { name, message } = item else {
                unreachable!();
            };
            panic!("shader {name}: {message}");
        };
        let vertex_spv = format!("{}.vert.spv", source.name);
        let fragment_spv = format!("{}.frag.spv", source.name);
        compile::compile_stage(
            &source.path,
            &source.vertex,
            "vertex",
            shaders,
            compile::Optimize::Release,
            &out_dir.join(&vertex_spv),
        )
        .unwrap_or_else(|message| panic!("shader {}: {message}", source.name));
        compile::compile_stage(
            &source.path,
            &source.fragment,
            "fragment",
            shaders,
            compile::Optimize::Release,
            &out_dir.join(&fragment_spv),
        )
        .unwrap_or_else(|message| panic!("shader {}: {message}", source.name));
        manifest.push_str(&format!(
            "    EmbeddedProgram {{ name: {name}, vertex_entry: {vertex}, fragment_entry: {fragment}, vertex: include_bytes!({vertex_spv}), fragment: include_bytes!({fragment_spv}) }},\n",
            name = rust_string(&source.name),
            vertex = rust_string(&source.vertex),
            fragment = rust_string(&source.fragment),
            vertex_spv = rust_string(&vertex_spv),
            fragment_spv = rust_string(&fragment_spv),
        ));
    }
    manifest.push_str("];\n");

    let mut file =
        fs::File::create(out_dir.join("shaders_gen.rs")).expect("create shader manifest");
    file.write_all(manifest.as_bytes())
        .expect("write shader manifest");
}

fn rust_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
