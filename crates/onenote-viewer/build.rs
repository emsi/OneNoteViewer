use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=ONENOTE_VIEWER_SOURCE_REVISION");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
    validate_symbolic_icons(Path::new("resources/icons/scalable/actions"));

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

fn validate_symbolic_icons(directory: &Path) {
    let entries = fs::read_dir(directory).expect("read symbolic icon directory");
    for entry in entries {
        let path = entry.expect("read symbolic icon entry").path();
        if path.extension().and_then(|value| value.to_str()) != Some("svg") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let source = fs::read_to_string(&path).expect("read symbolic icon");
        for unsupported in ["<line ", "<polyline ", "<polygon ", "<ellipse ", "<text "] {
            assert!(
                !source.contains(unsupported),
                "{} uses unsupported GTK symbolic SVG element {}",
                path.display(),
                unsupported.trim()
            );
        }
        for line in source.lines().map(str::trim_start) {
            if ["<path ", "<circle ", "<rect "]
                .iter()
                .any(|primitive| line.starts_with(primitive))
            {
                let foreground_fill = line.contains("foreground-fill");
                let transparent_stroke =
                    line.contains("foreground-stroke") && line.contains("transparent-fill");
                assert!(
                    foreground_fill || transparent_stroke,
                    "{} has a graphic primitive without a GTK symbolic fill, or a stroke \
                     primitive without transparent-fill",
                    path.display()
                );
            }
        }
    }
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
