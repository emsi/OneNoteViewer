use crate::Reader;
use crate::debug::DebugOutput;
use crate::errors::Result;
use crate::onestore::shared::property::{PropertyId, PropertyValue};
use crate::utils::Utf16ToString;
use std::fmt;

/// A property set.
///
/// See [\[MS-ONESTORE\] 2.6.7].
///
/// [\[MS-ONESTORE\] 2.6.7]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-onestore/88a64c18-f815-4ebc-8590-ddd432024ab9
#[derive(Clone, Default)]
pub(crate) struct PropertySet {
    values: Vec<(u32, PropertyValue)>,
}

impl fmt::Debug for PropertySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for (key, value) in &self.values {
            let key = format!("{:#x}", key);
            let value = format_property_value(value);
            map.entry(
                &DebugOutput::from(key.as_str()),
                &DebugOutput::from(value.as_str()),
            );
        }
        map.finish()
    }
}

fn format_property_value(value: &PropertyValue) -> String {
    if let PropertyValue::Vec(bytes) = value {
        // OneNote strings are commonly stored as UTF-16-encoded byte vecs.
        // Try decoding; if the result contains any ASCII letter/space treat
        // it as a string for readability while still showing the raw bytes.
        if let Ok(decoded) = bytes.as_slice().utf16_to_string() {
            if !decoded.is_empty()
                && decoded
                    .chars()
                    .any(|c| c.is_ascii_whitespace() || c.is_ascii_alphanumeric())
            {
                return format!("{:?} ({:?})", decoded, bytes);
            }
        }
        return format!("{:?}", bytes);
    }
    // Compact single-line representation keeps long debug dumps readable.
    format!("{:?}", value)
}

impl PropertySet {
    pub(crate) fn parse(reader: Reader) -> Result<PropertySet> {
        let count = reader.get_u16()?;

        let property_ids: Vec<_> = (0..count)
            .map(|_| PropertyId::parse(reader))
            .collect::<Result<_>>()?;

        let values = property_ids
            .into_iter()
            .map(|id| Ok((id.id(), PropertyValue::parse(id, reader)?)))
            .collect::<Result<_>>()?;

        Ok(PropertySet { values })
    }

    pub(crate) fn get(&self, id: PropertyId) -> Option<&PropertyValue> {
        self.values
            .iter()
            .find(|(key, _)| *key == id.id())
            .map(|(_, value)| value)
    }

    pub(crate) fn index(&self, id: PropertyId) -> Option<usize> {
        self.values.iter().position(|(key, _)| *key == id.id())
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &PropertyValue> {
        self.values.iter().map(|(_, value)| value)
    }

    pub(crate) fn values_with_index(&self) -> impl Iterator<Item = (usize, &PropertyValue)> {
        self.values
            .iter()
            .enumerate()
            .map(|(idx, (_, value))| (idx, value))
    }
}
