fn main() {
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/onenote-viewer.gresource.xml",
        "onenote-viewer.gresource",
    );
}
