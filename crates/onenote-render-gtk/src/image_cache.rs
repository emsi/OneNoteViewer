use gtk::gdk;
use gtk::glib;
use gtk::prelude::Cast;
use image::{ImageReader, Limits};
use onenote_core::{ResourceId, ResourceStore};
use std::io::Cursor;
use std::sync::Arc;

pub(crate) const MAX_ENCODED_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_DECODED_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_IMAGE_DIMENSION: u32 = 16_384;
pub(crate) const MAX_TEXTURE_CACHE_BYTES: usize = 128 * 1024 * 1024;

pub(crate) struct DecodedImage {
    pub(crate) id: ResourceId,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn decode(resources: &ResourceStore, id: &ResourceId) -> Option<DecodedImage> {
    let encoded = resources.read_limited(id, MAX_ENCODED_IMAGE_BYTES).ok()?;
    let mut reader = ImageReader::new(Cursor::new(encoded))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_IMAGE_BYTES);
    reader.limits(limits);
    let decoded = reader.decode().ok()?.into_rgba8();
    let width = i32::try_from(decoded.width()).ok()?;
    let height = i32::try_from(decoded.height()).ok()?;
    Some(DecodedImage {
        id: id.clone(),
        width,
        height,
        bytes: decoded.into_raw(),
    })
}

pub(crate) fn texture(decoded: DecodedImage) -> (ResourceId, gdk::Texture, usize) {
    let size = decoded.bytes.len();
    let stride = usize::try_from(decoded.width).unwrap_or(0) * 4;
    let bytes = glib::Bytes::from_owned(decoded.bytes);
    let texture = gdk::MemoryTexture::new(
        decoded.width,
        decoded.height,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        stride,
    )
    .upcast();
    (decoded.id, texture, size)
}

pub(crate) fn spawn_decode(
    resources: Arc<ResourceStore>,
    id: ResourceId,
    callback: impl FnOnce(ResourceId, Option<DecodedImage>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let decoded = decode(&resources, &id);
        callback(id, decoded);
    });
}
