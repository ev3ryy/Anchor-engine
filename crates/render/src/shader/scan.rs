use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub struct GraphicsEntries {
    pub vertex: String,
    pub fragment: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProgramKind {
    Module,
    Graphics(GraphicsEntries),
}

#[derive(Debug)]
pub struct SourceFile {
    pub name: String,
    pub path: PathBuf,
    pub vertex: String,
    pub fragment: String,
}

#[derive(Debug)]
pub enum Listed {
    Program(SourceFile),
    Failed { name: String, message: String },
}

pub fn scan(source: &str) -> Result<ProgramKind, String> {
    let cleaned = strip_comments(source);
    let mut vertex = None;
    let mut fragment = None;
    let mut rest = cleaned.as_str();
    while let Some((stage, after_attr)) = next_attribute(rest)? {
        let (name, after_name) = function_name(after_attr)?;
        match stage {
            "vertex" => assign_stage(&mut vertex, name, "vertex")?,
            "fragment" => assign_stage(&mut fragment, name, "fragment")?,
            "compute" => return Err("compute shaders are not supported".into()),
            other => return Err(format!("unsupported shader stage {other}")),
        }
        rest = after_name;
    }
    match (vertex, fragment) {
        (None, None) => Ok(ProgramKind::Module),
        (Some(vertex), Some(fragment)) => {
            Ok(ProgramKind::Graphics(GraphicsEntries { vertex, fragment }))
        }
        (Some(_), None) => Err("missing fragment entry point".into()),
        (None, Some(_)) => Err("missing vertex entry point".into()),
    }
}

pub fn list_graphics(dir: &Path) -> Result<Vec<Listed>, String> {
    let mut paths = Vec::new();
    let entries =
        fs::read_dir(dir).map_err(|err| format!("failed to read {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read {}: {err}", dir.display()))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "slang") {
            paths.push(path);
        }
    }
    paths.sort();

    let mut listed = Vec::new();
    let mut seen = Vec::new();
    for path in paths {
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("shader")
            .to_string();
        if !is_safe_name(&name) {
            listed.push(Listed::Failed {
                name: name.clone(),
                message: "shader file name must be ASCII alphanumeric or underscore".into(),
            });
            continue;
        }
        if seen.iter().any(|existing: &String| existing == &name) {
            listed.push(Listed::Failed {
                name: name.clone(),
                message: "duplicate shader name".into(),
            });
            continue;
        }
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(err) => {
                listed.push(Listed::Failed {
                    name,
                    message: err.to_string(),
                });
                continue;
            }
        };
        match scan(&source) {
            Ok(ProgramKind::Module) => {}
            Ok(ProgramKind::Graphics(entries)) => {
                seen.push(name.clone());
                listed.push(Listed::Program(SourceFile {
                    name,
                    path,
                    vertex: entries.vertex,
                    fragment: entries.fragment,
                }));
            }
            Err(message) => listed.push(Listed::Failed { name, message }),
        }
    }
    Ok(listed)
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn assign_stage(slot: &mut Option<String>, name: String, stage: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("duplicate {stage} entry point"));
    }
    *slot = Some(name);
    Ok(())
}

fn next_attribute(source: &str) -> Result<Option<(&str, &str)>, String> {
    let Some(index) = source.find("[shader(") else {
        return Ok(None);
    };
    let Some((stage, rest)) = parse_attribute(&source[index..]) else {
        return Err("malformed [shader(...)] attribute".into());
    };
    if stage.is_empty() {
        return Err("shader stage name is empty".into());
    }
    Ok(Some((stage, rest)))
}

fn parse_attribute(source: &str) -> Option<(&str, &str)> {
    let rest = source.trim_start().strip_prefix("[shader(")?;
    let rest = rest.trim_start().strip_prefix('"')?;
    let (stage, rest) = rest.split_once('"')?;
    let rest = rest.trim_start().strip_prefix(')')?;
    let rest = rest.trim_start().strip_prefix(']')?;
    Some((stage, rest))
}

fn function_name(source: &str) -> Result<(String, &str), String> {
    let mut rest = source;
    loop {
        rest = rest.trim_start();
        if rest.starts_with('[') {
            rest = skip_brackets(rest)?;
            continue;
        }
        break;
    }
    let paren = rest
        .find('(')
        .ok_or_else(|| "entry point function not found".to_string())?;
    let name =
        last_ident(&rest[..paren]).ok_or_else(|| "entry point name not found".to_string())?;
    Ok((name, &rest[paren..]))
}

fn skip_brackets(source: &str) -> Result<&str, String> {
    let mut depth = 0i32;
    for (index, ch) in source.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&source[index + ch.len_utf8()..]);
                }
            }
            _ => {}
        }
    }
    Err("unclosed attribute".into())
}

fn last_ident(source: &str) -> Option<String> {
    let mut current = String::new();
    let mut last = None;
    let mut in_ident = false;
    for ch in source.chars() {
        let continues = ch.is_ascii_alphanumeric() || ch == '_';
        if continues {
            if !in_ident {
                if ch.is_ascii_digit() {
                    continue;
                }
                current.clear();
                in_ident = true;
            }
            current.push(ch);
        } else if in_ident {
            last = Some(std::mem::take(&mut current));
            in_ident = false;
        }
    }
    if in_ident {
        last = Some(current);
    }
    last
}

fn strip_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '/' && chars.get(index + 1) == Some(&'/') {
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                index += 1;
            }
            index = (index + 2).min(chars.len());
            continue;
        }
        if chars[index] == '"' {
            out.push('"');
            index += 1;
            while index < chars.len() && chars[index] != '"' {
                out.push(chars[index]);
                if chars[index] == '\\' && index + 1 < chars.len() {
                    index += 1;
                    if index < chars.len() {
                        out.push(chars[index]);
                    }
                }
                index += 1;
            }
            if index < chars.len() {
                out.push('"');
                index += 1;
            }
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{ProgramKind, scan};

    #[test]
    fn scans_triangle_entries() {
        let source = include_str!("../../shaders/triangle.slang");
        match scan(source).expect("scan") {
            ProgramKind::Graphics(entries) => {
                assert_eq!(entries.vertex, "vertexMain");
                assert_eq!(entries.fragment, "fragmentMain");
            }
            ProgramKind::Module => panic!("triangle.slang has entry points"),
        }
    }

    #[test]
    fn ignores_commented_entry_points() {
        let source = r#"
            // [shader("vertex")]
            // float4 fake(uint id : SV_VertexID) : SV_Position { return 0; }
            /* [shader("fragment")]
               float4 also_fake() : SV_Target { return 0; } */
            [shader("vertex")]
            float4 vertexMain(uint id : SV_VertexID) : SV_Position { return 0; }

            [shader("fragment")]
            [extra]
            float4 fragmentMain() : SV_Target { return 1; }
        "#;
        match scan(source).expect("scan") {
            ProgramKind::Graphics(entries) => {
                assert_eq!(entries.vertex, "vertexMain");
                assert_eq!(entries.fragment, "fragmentMain");
            }
            ProgramKind::Module => panic!("expected graphics program"),
        }
    }

    #[test]
    fn reports_a_missing_stage() {
        let source = r#"
            [shader("vertex")]
            void vertexMain(uint id : SV_VertexID) {}
        "#;
        let error = scan(source).expect_err("missing fragment");
        assert!(error.contains("fragment"), "{error}");
    }

    #[test]
    fn skips_modules_without_entry_points() {
        let source = "void helper() {}\n";
        assert_eq!(scan(source).expect("scan"), ProgramKind::Module);
    }
}
