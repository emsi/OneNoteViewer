use gtk::gdk;
use gtk::glib;
use gtk::prelude::Cast;
use image::{ImageReader, Limits};
use onenote_core::{Error, ResourceId, ResourceStatus, ResourceStore};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImageDecodeFailure {
    Unavailable,
    CannotDisplay,
}

pub(crate) fn decode(
    resources: &ResourceStore,
    id: &ResourceId,
) -> Result<DecodedImage, ImageDecodeFailure> {
    if resources.status(id).ok() != Some(ResourceStatus::Available) {
        return Err(ImageDecodeFailure::Unavailable);
    }
    let encoded = resources
        .read_limited(id, MAX_ENCODED_IMAGE_BYTES)
        .map_err(|error| match error {
            Error::ResourceTooLarge { .. } => ImageDecodeFailure::CannotDisplay,
            _ => ImageDecodeFailure::Unavailable,
        })?;
    decode_encoded(id, encoded)
}

fn decode_encoded(id: &ResourceId, encoded: Vec<u8>) -> Result<DecodedImage, ImageDecodeFailure> {
    let mut reader = ImageReader::new(Cursor::new(encoded))
        .with_guessed_format()
        .map_err(|_| ImageDecodeFailure::CannotDisplay)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_IMAGE_BYTES);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|_| ImageDecodeFailure::CannotDisplay)?
        .into_rgba8();
    let width = i32::try_from(decoded.width()).map_err(|_| ImageDecodeFailure::CannotDisplay)?;
    let height = i32::try_from(decoded.height()).map_err(|_| ImageDecodeFailure::CannotDisplay)?;
    Ok(DecodedImage {
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
    callback: impl FnOnce(ResourceId, Result<DecodedImage, ImageDecodeFailure>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let decoded = decode(&resources, &id);
        callback(id, decoded);
    });
}

#[cfg(test)]
mod tests {
    use super::{decode_encoded, ImageDecodeFailure};
    use image::{DynamicImage, ImageFormat};
    use onenote_core::ResourceId;
    use std::io::Cursor;

    #[test]
    fn decodes_a_supported_picture() {
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(1, 1)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();

        let decoded = decode_encoded(&ResourceId::new("valid"), encoded.into_inner()).unwrap();

        assert_eq!((decoded.width, decoded.height), (1, 1));
        assert_eq!(decoded.bytes.len(), 4);
    }

    #[test]
    fn invalid_picture_data_has_a_display_failure() {
        let result = decode_encoded(&ResourceId::new("invalid"), b"not an image".to_vec());

        assert!(matches!(result, Err(ImageDecodeFailure::CannotDisplay)));
    }
}
