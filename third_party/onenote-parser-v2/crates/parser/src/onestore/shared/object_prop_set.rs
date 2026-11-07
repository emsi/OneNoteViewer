use crate::Reader;
use crate::errors::Result;
use crate::one::property::PropertyType;
use crate::onestore::shared::compact_id::CompactId;
use crate::onestore::shared::object_stream_header::ObjectStreamHeader;
use crate::onestore::shared::prop_set::PropertySet;
use crate::onestore::shared::property::{PropertyId, PropertyValue};

/// An object's properties.
///
/// See [\[MS-ONESTORE\] 2.1.1].
///
/// [\[MS-ONESTORE\] 2.1.1]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-onestore/e9fb4b61-5128-45dd-9a96-6bad6f11dc18
#[derive(Debug, Clone, Default)]
pub(crate) struct ObjectPropSet {
    pub(crate) object_ids: Vec<CompactId>,
    pub(crate) object_space_ids: Vec<CompactId>,
    pub(crate) context_ids: Vec<CompactId>,
    pub(crate) properties: PropertySet,
}

impl ObjectPropSet {
    pub(crate) fn object_ids(&self) -> &[CompactId] {
        &self.object_ids
    }

    pub(crate) fn object_space_ids(&self) -> &[CompactId] {
        &self.object_space_ids
    }

    pub(crate) fn context_ids(&self) -> &[CompactId] {
        &self.context_ids
    }

    pub(crate) fn properties(&self) -> &PropertySet {
        &self.properties
    }
}

impl ObjectPropSet {
    pub(crate) fn parse(reader: Reader) -> Result<ObjectPropSet> {
        let header = ObjectStreamHeader::parse(reader)?;
        let object_ids = Self::parse_compact_ids(reader, header.count)?;

        let mut object_space_ids = vec![];
        let mut context_ids = vec![];

        if !header.osid_stream_not_present {
            let header = ObjectStreamHeader::parse(reader)?;

            object_space_ids = Self::parse_compact_ids(reader, header.count)?;

            if header.extended_streams_present {
                let header = ObjectStreamHeader::parse(reader)?;
                context_ids = Self::parse_compact_ids(reader, header.count)?;
            };
        }

        let properties = PropertySet::parse(reader)?;

        Ok(ObjectPropSet {
            object_ids,
            object_space_ids,
            context_ids,
            properties,
        })
    }

    /// `count` comes straight from the file; grow the `Vec` as elements
    /// actually parse instead of pre-allocating an attacker-controlled count.
    fn parse_compact_ids(reader: Reader, count: u32) -> Result<Vec<CompactId>> {
        let mut ids = Vec::new();
        for _ in 0..count {
            ids.push(CompactId::parse(reader)?);
        }
        Ok(ids)
    }

    pub(crate) fn get(&self, prop_type: PropertyType) -> Option<&PropertyValue> {
        self.properties.get(PropertyId::new(prop_type as u32))
    }
}
