use shadow_rs::ShadowBuilder;
use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ShadowBuilder::builder().build()?;
    compose_grammars()?;
    Ok(())
}

/// Concatenate every top-level ABNF file in `grammar/` with the shared
/// `_unicode.abnf` file and write the result to `grammar/.generated/`.
///
/// Files whose basename starts with `_` are treated as includes and are not
/// emitted on their own. Generated files are referenced via
/// `#[grammar(file = "grammar/.generated/<top>.abnf")]`; the directory is
/// kept out of version control (`.gitignore`). `OUT_DIR` is not used because
/// `static-regular-grammar` resolves `file` against `CARGO_MANIFEST_DIR`
/// only, and its proc-macro sees `#[doc = include_str!(...)]` as raw tokens
/// (rejecting non-literal doc attributes).
fn compose_grammars() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let grammar_dir = manifest.join("grammar");
    let unicode = fs::read_to_string(grammar_dir.join("_unicode.abnf"))?;

    let out_root = grammar_dir.join(".generated");
    fs::create_dir_all(&out_root)?;

    println!("cargo:rerun-if-changed=grammar");

    for entry in fs::read_dir(&grammar_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("abnf") {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };
        if name.starts_with('_') {
            continue;
        }
        compose_one(&path, &unicode, &out_root.join(name))?;
    }
    Ok(())
}

fn compose_one(top: &Path, unicode: &str, out: &Path) -> std::io::Result<()> {
    let top_src = fs::read_to_string(top)?;
    let mut composed = String::with_capacity(top_src.len() + unicode.len() + 1);
    composed.push_str(&top_src);
    if !top_src.ends_with('\n') {
        composed.push('\n');
    }
    composed.push_str(unicode);
    if let Ok(existing) = fs::read_to_string(out) {
        if existing == composed {
            return Ok(());
        }
    }
    fs::write(out, composed)
}
