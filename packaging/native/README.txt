OneNote Viewer native preview build
===================================

This archive contains an optimized, dynamically linked Linux executable.
It does not require Rust, Cargo, compiler headers, or pkg-config.

Run:

    ./onenote-viewer /path/to/notebook

The target host must provide GTK 4.14 or newer and its runtime dependencies.
On Ubuntu 24.04:

    sudo apt install libgtk-4-1 libgraphene-1.0-0

Use the Flatpak bundle instead when testing across different distributions.
The Flatpak supplies a consistent GTK runtime.

.onepkg onboarding additionally requires 7zz or 7z. Opening an already
extracted .one/.onetoc2 notebook tree does not require 7-Zip.
