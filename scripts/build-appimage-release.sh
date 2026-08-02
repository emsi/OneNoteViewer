#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

for command in cargo curl file glib-compile-schemas install pkg-config sha256sum; do
    if ! command -v "$command" >/dev/null 2>&1; then
        printf 'missing AppImage build command: %s\n' "$command" >&2
        printf '%s\n' \
            'Ubuntu: sudo apt install adwaita-icon-theme curl file libgtk-4-dev librsvg2-common patchelf shared-mime-info' >&2
        exit 1
    fi
done

case "$(uname -m)" in
    x86_64) arch=x86_64 ;;
    *)
        printf 'unsupported AppImage build architecture: %s\n' "$(uname -m)" >&2
        exit 1
        ;;
esac

version=$(sed -n '
    /^\[workspace\.package\]$/,/^\[/ {
        s/^version = "\([^"]*\)"/\1/p
    }
' Cargo.toml)
if [ -z "$version" ]; then
    printf '%s\n' 'could not determine workspace version' >&2
    exit 1
fi

linuxdeploy_url=https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage
linuxdeploy_sha256=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
appimagetool_url=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
appimagetool_sha256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
appimage_runtime_url=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
appimage_runtime_sha256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
sevenzip_binary_url=https://github.com/ip7z/7zip/releases/download/26.02/7z2602-linux-x64.tar.xz
sevenzip_binary_sha256=41aaba7b1235304ab5aa0624530c67ae829496cd29e875925271efdccc28c03e
sevenzip_source_url=https://github.com/ip7z/7zip/releases/download/26.02/7z2602-src.tar.xz
sevenzip_source_sha256=cf967c98bca02a4b8b16375f441825a8e141362f14be1969bbec8e1ca0bff9dd

build_root=target/appimage
appdir="$build_root/OneNoteViewer.AppDir"
downloads="$build_root/downloads"
linuxdeploy="$downloads/linuxdeploy-x86_64.AppImage"
appimagetool="$downloads/appimagetool-x86_64.AppImage"
appimage_runtime="$downloads/runtime-x86_64"
sevenzip_binary="$downloads/7z2602-linux-x64.tar.xz"
sevenzip_source="$downloads/7z2602-src.tar.xz"
bundle="dist/OneNoteViewer-${version}-${arch}.AppImage"

download_verified() {
    url=$1
    output=$2
    expected=$3
    if [ -f "$output" ] && printf '%s  %s\n' "$expected" "$output" | sha256sum -c - >/dev/null 2>&1; then
        return
    fi
    curl --fail --location --retry 3 --output "$output.download" "$url"
    printf '%s  %s\n' "$expected" "$output.download" | sha256sum -c -
    mv "$output.download" "$output"
}

mkdir -p "$downloads" dist
download_verified "$linuxdeploy_url" "$linuxdeploy" "$linuxdeploy_sha256"
download_verified "$appimagetool_url" "$appimagetool" "$appimagetool_sha256"
download_verified "$appimage_runtime_url" "$appimage_runtime" "$appimage_runtime_sha256"
download_verified "$sevenzip_binary_url" "$sevenzip_binary" "$sevenzip_binary_sha256"
download_verified "$sevenzip_source_url" "$sevenzip_source" "$sevenzip_source_sha256"
chmod +x "$linuxdeploy" "$appimagetool"

cargo fetch --locked
cargo build --offline --frozen --locked --release -p onenote-viewer

rm -rf "$appdir"
mkdir -p \
    "$appdir/usr/bin" \
    "$appdir/usr/lib" \
    "$appdir/usr/share/doc/io.github.emsi.OneNoteViewer" \
    "$appdir/usr/share/licenses/io.github.emsi.OneNoteViewer" \
    "$appdir/usr/share/sources/io.github.emsi.OneNoteViewer"

sevenzip_dir="$build_root/7zip"
rm -rf "$sevenzip_dir"
mkdir -p "$sevenzip_dir"
tar -xJf "$sevenzip_binary" -C "$sevenzip_dir"
install -m 0755 "$sevenzip_dir/7zzs" "$appdir/usr/bin/7zz"
install -m 0644 \
    "$sevenzip_dir/License.txt" \
    "$appdir/usr/share/licenses/io.github.emsi.OneNoteViewer/7-ZIP-LICENSE.txt"
install -m 0644 \
    "$sevenzip_source" \
    "$appdir/usr/share/sources/io.github.emsi.OneNoteViewer/7z2602-src.tar.xz"

install -m 0644 LICENSE \
    "$appdir/usr/share/licenses/io.github.emsi.OneNoteViewer/LICENSE"
install -m 0644 SOURCE-CODE.md THIRD-PARTY-NOTICES.md THIRD-PARTY-LICENSES.html \
    "$appdir/usr/share/doc/io.github.emsi.OneNoteViewer/"
install -m 0644 crates/onenote-viewer/resources/LUCIDE-LICENSE \
    "$appdir/usr/share/licenses/io.github.emsi.OneNoteViewer/LUCIDE-LICENSE"
install -m 0644 third_party/onenote.rs/LICENSE \
    "$appdir/usr/share/licenses/io.github.emsi.OneNoteViewer/ONENOTE-PARSER-MPL-2.0.txt"

APPIMAGE_EXTRACT_AND_RUN=1 "$linuxdeploy" \
    --appdir "$appdir" \
    --executable target/release/onenote-viewer \
    --desktop-file packaging/flatpak/io.github.emsi.OneNoteViewer.desktop \
    --icon-file crates/onenote-viewer/resources/icons/scalable/apps/io.github.emsi.OneNoteViewer.svg \
    --custom-apprun packaging/appimage/AppRun

pixbuf_module_dir=$(pkg-config --variable=gdk_pixbuf_moduledir gdk-pixbuf-2.0)
pixbuf_query_loaders=$(pkg-config --variable=gdk_pixbuf_query_loaders gdk-pixbuf-2.0)
schemas_dir=$(pkg-config --variable=schemasdir gio-2.0)
app_pixbuf_dir="$appdir/usr/lib/gdk-pixbuf-2.0/2.10.0"
if [ ! -f "$pixbuf_module_dir/libpixbufloader-svg.so" ]; then
    printf '%s\n' \
        'missing SVG image loader; install librsvg2-common before building the AppImage' >&2
    exit 1
fi
for data_dir in /usr/share/gtk-4.0 /usr/share/icons/Adwaita /usr/share/mime "$schemas_dir"; do
    if [ ! -d "$data_dir" ]; then
        printf 'missing AppImage runtime data directory: %s\n' "$data_dir" >&2
        exit 1
    fi
done
mkdir -p "$app_pixbuf_dir/loaders" "$appdir/usr/share/glib-2.0/schemas"
install -m 0755 "$pixbuf_query_loaders" "$app_pixbuf_dir/"
for loader in "$pixbuf_module_dir"/*.so; do
    install -m 0755 "$loader" "$app_pixbuf_dir/loaders/"
done
for schema in "$schemas_dir"/*.xml "$schemas_dir"/*.override; do
    if [ -f "$schema" ]; then
        install -m 0644 "$schema" "$appdir/usr/share/glib-2.0/schemas/"
    fi
done
glib-compile-schemas "$appdir/usr/share/glib-2.0/schemas"
cp -a /usr/share/gtk-4.0 /usr/share/mime "$appdir/usr/share/"
mkdir -p "$appdir/usr/share/icons"
cp -a /usr/share/icons/Adwaita "$appdir/usr/share/icons/"

APPIMAGE_EXTRACT_AND_RUN=1 "$linuxdeploy" \
    --appdir "$appdir" \
    --deploy-deps-only "$app_pixbuf_dir"

"$appdir/usr/bin/7zz" i | grep 'Cab' >/dev/null
test -x "$appdir/usr/bin/onenote-viewer"
test -f "$appdir/usr/share/sources/io.github.emsi.OneNoteViewer/7z2602-src.tar.xz"

rm -f "$bundle" "$bundle.sha256"
ARCH="$arch" APPIMAGE_EXTRACT_AND_RUN=1 "$appimagetool" \
    --no-appstream \
    --runtime-file "$appimage_runtime" \
    "$appdir" \
    "$bundle"
chmod +x "$bundle"
(cd dist && sha256sum "$(basename "$bundle")" >"$(basename "$bundle").sha256")
printf 'Created %s\n' "$root/$bundle"
