use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=web/dist");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is not set"));
    let target_dir = out_dir.join("embedded-web");
    if target_dir.exists() {
        let _ = fs::remove_dir_all(&target_dir);
    }
    fs::create_dir_all(&target_dir).expect("Failed to create embedded-web output directory");

    let source_dir = Path::new("web").join("dist");
    if source_dir.exists() {
        copy_dir_recursive(&source_dir, &target_dir).expect("Failed to copy web/dist");
    } else {
        let placeholder = "<!doctype html><html><head><meta charset=\"utf-8\"/><title>LinxPanel</title></head><body><h3>Embedded web resources are missing.</h3><p>Please run <code>npm run build</code> in <code>web/</code> before building backend.</p></body></html>";
        fs::write(target_dir.join("index.html"), placeholder)
            .expect("Failed to write placeholder index.html");
        println!("cargo:warning=web/dist not found, embedding placeholder page only");
    }
}

fn copy_dir_recursive(source: &Path, target: &Path) -> std::io::Result<()> {
    if !target.exists() {
        fs::create_dir_all(target)?;
    }

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let entry_path = entry.path();
        let target_path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry_path, &target_path)?;
        } else {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry_path, target_path)?;
        }
    }

    Ok(())
}
