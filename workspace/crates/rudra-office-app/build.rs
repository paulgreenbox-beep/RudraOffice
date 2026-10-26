use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git_text(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn git_status(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn normalize(value: &str, fallback: &str) -> String {
    let normalized: String = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .take(64)
        .collect();
    if normalized.is_empty() {
        fallback.to_owned()
    } else {
        normalized
    }
}

fn project_root() -> PathBuf {
    let manifest =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("Cargo provides CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("rudra-office-app must remain under workspace/crates")
        .to_path_buf()
}

fn main() {
    println!("cargo:rerun-if-env-changed=RUDRA_RELEASE_SOURCE_REVISION");
    println!("cargo:rerun-if-env-changed=RUDRA_RELEASE_SOURCE_STATE");

    let root = project_root();
    let revision = env::var("RUDRA_RELEASE_SOURCE_REVISION")
        .ok()
        .map(|value| normalize(&value, "nogit"))
        .or_else(|| {
            git_text(&root, &["rev-parse", "--short=12", "HEAD"])
                .map(|value| normalize(&value, "nogit"))
        })
        .unwrap_or_else(|| "nogit".to_owned());
    let state = env::var("RUDRA_RELEASE_SOURCE_STATE")
        .ok()
        .map(|value| normalize(&value, "untracked"))
        .unwrap_or_else(|| match git_status(&root) {
            Some(status) if status.is_empty() => "clean".to_owned(),
            Some(_) => "dirty".to_owned(),
            None => "untracked".to_owned(),
        });
    let package_version = env::var("CARGO_PKG_VERSION").expect("Cargo provides CARGO_PKG_VERSION");
    let build_id = format!("rudra-office-viewer-{package_version}+{revision}.{state}");

    println!("cargo:rustc-env=RUDRA_PRODUCT_BUILD_ID={build_id}");
    println!("cargo:rustc-env=RUDRA_SOURCE_REVISION={revision}");
    println!("cargo:rustc-env=RUDRA_SOURCE_STATE={state}");
}
