use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=ONENOTE_VIEWER_SOURCE_REVISION");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");

    let revision = std::env::var("ONENOTE_VIEWER_SOURCE_REVISION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(source_revision);
    println!("cargo:rustc-env=ONENOTE_VIEWER_SOURCE_REVISION={revision}");

    glib_build_tools::compile_resources(
        &["resources"],
        "resources/onenote-viewer.gresource.xml",
        "onenote-viewer.gresource",
    );
}

fn source_revision() -> String {
    let root = "../..";
    let output = Command::new("git")
        .args(["-C", root, "rev-parse", "--verify", "HEAD"])
        .output();
    let Ok(output) = output else {
        return "unknown".to_owned();
    };
    if !output.status.success() {
        return "unknown".to_owned();
    }

    let mut revision = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let dirty = Command::new("git")
        .args(["-C", root, "status", "--porcelain", "--untracked-files=no"])
        .output()
        .is_ok_and(|output| output.status.success() && !output.stdout.is_empty());
    if dirty {
        revision.push_str("-dirty");
    }
    revision
}
