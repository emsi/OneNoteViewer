use crate::Reader;
use crate::errors::Result;
use crate::fsshttpb::data::compact_u64::CompactU64;
use crate::fsshttpb::data::object_types::ObjectType;
use crate::fsshttpb::data::stream_object::ObjectHeader;
use crate::fsshttpb::data_element::DataElement;
use crate::onestore::shared::file_blob::FileBlob;
use std::fmt;

/// An object data blob.
///
/// See [\[MS-FSSHTTPB\] 2.2.1.12.8]
///
/// [\[MS-FSSHTTPB\] 2.2.1.12.8]: https://docs.microsoft.com/en-us/openspecs/sharepoint_protocols/ms-fsshttpb/d36dd2b4-bad1-441b-93c7-adbe3069152c
pub(crate) struct ObjectDataBlob(FileBlob);

impl ObjectDataBlob {
    pub(crate) fn value(&self) -> FileBlob {
        self.0.clone()
    }
}

impl fmt::Debug for ObjectDataBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectDataBlob({} bytes)", self.0.size())
    }
}

impl DataElement {
    pub(crate) fn parse_object_data_blob(reader: Reader) -> Result<ObjectDataBlob> {
        ObjectHeader::try_parse(reader, ObjectType::ObjectDataBlob)?;

        // Inlined BinaryItem so the blob's bytes stay as a FileSource-backed
        // reference instead of being copied into a Vec. Embedded files /
        // images downstream see the same FileSource the parser is reading
        // from; `EmbeddedFile::read()` then pulls bytes on demand without
        // ever materialising the full payload.
        let size = CompactU64::parse(reader)?.value();
        let blob = FileBlob::from_source(reader.source(), reader.position(), size);
        reader.advance(size as usize)?;

        ObjectHeader::try_parse_end_8(reader, ObjectType::DataElement)?;

        Ok(ObjectDataBlob(blob))
    }
}
