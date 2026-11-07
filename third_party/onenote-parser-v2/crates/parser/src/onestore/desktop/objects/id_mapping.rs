use crate::errors::Result;
use crate::fsshttpb::data::exguid::ExGuid;
use crate::onestore::shared::compact_id::CompactId;
use crate::shared::guid::Guid;
use std::{collections::HashMap, fmt::Debug};

/// A subset of the global ID table that applies to a particular range of nodes.
/// Maps from GUID index -> GUID. The other part of the full ID is stored directly
/// in each CompactId.
#[derive(Clone)]
pub(crate) struct IdMapping(HashMap<u32, Guid>);

impl Debug for IdMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IdMapping(entry count: {:})", self.0.len())
    }
}

impl IdMapping {
    pub(crate) fn new() -> Self {
        IdMapping(HashMap::new())
    }

    pub(crate) fn resolve_id(&self, id: &CompactId) -> Result<ExGuid> {
        let guid = self.0.get(&id.guid_index).ok_or(parser_error!(
            ResolutionFailed,
            "Missing mapping for ID (index: {})",
            id.guid_index
        ))?;

        Ok(ExGuid::from_guid(*guid, id.n.into()))
    }

    pub(crate) fn add_mapping(&mut self, guid_index: u32, guid: Guid) {
        self.0.insert(guid_index, guid);
    }

    /// Looks up the raw GUID stored at a given index, if any.
    ///
    /// Used to resolve [`GlobalIdTableEntry2FNDX`] references, which inherit a
    /// GUID from a dependency revision's global identification table.
    ///
    /// [`GlobalIdTableEntry2FNDX`]: crate::onestore::desktop::file_node::FileNodeData::GlobalIdTableEntry2FNDX
    pub(crate) fn guid_for_index(&self, guid_index: u32) -> Option<Guid> {
        self.0.get(&guid_index).copied()
    }

    /// Merges all entries from another mapping into this one.
    pub(crate) fn merge(&mut self, other: &IdMapping) {
        self.0.extend(other.0.iter().map(|(k, v)| (*k, *v)));
    }
}
