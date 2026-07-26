use crate::errors::Result;
use crate::fsshttpb::data::exguid::ExGuid;
use crate::onestore::desktop::file_node::FileNodeData;
use crate::onestore::desktop::file_structure::FileNodeDataIterator;
use crate::onestore::desktop::objects::id_mapping::IdMapping;
use crate::onestore::shared::compact_id::CompactId;

/// Lower-level structure for mapping local `CompactId`s to global `ExGuid`s. Applies to a
/// particular region of a OneStore file.
///
/// In `.onetoc2` files, `GlobalIdTable`s may depend on other `GlobalIdTable`s: a
/// [`GlobalIdTableEntry2FNDX`] entry inherits a GUID from the global identification table of the
/// revision's dependency revision (see [MS-ONESTORE] 2.5.11). Such entries are resolved eagerly
/// against `parent` while parsing, so the resulting `id_map` is self-contained.
///
/// See [\[MS-ONESTORE\] 2.1.3](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-onestore/a243bd78-6cfd-4e18-96c7-e8c2095ce6b0)
///
/// [`GlobalIdTableEntry2FNDX`]: FileNodeData::GlobalIdTableEntry2FNDX
#[derive(Debug, Clone)]
pub(crate) struct GlobalIdTable {
    pub(crate) id_map: IdMapping,
}

impl GlobalIdTable {
    /// Parses a global identification table, resolving any dependency-revision references against
    /// `parent` (the merged `id_map` of the revision this one depends on, if any).
    pub(crate) fn try_parse(
        iterator: &mut FileNodeDataIterator,
        parent: Option<&IdMapping>,
    ) -> Result<Option<Self>> {
        let next = iterator.peek();

        match next {
            Some(
                FileNodeData::GlobalIdTableStart2FND | FileNodeData::GlobalIdTableStartFNDX(_),
            ) => Ok(Some(GlobalIdTable::parse(iterator, parent)?)),
            _ => Ok(None),
        }
    }

    fn parse(iterator: &mut FileNodeDataIterator, parent: Option<&IdMapping>) -> Result<Self> {
        // Skip the start node
        iterator.next();

        let mut id_map = IdMapping::new();

        for node in iterator {
            match node {
                FileNodeData::GlobalIdTableEndFNDX => {
                    break;
                }
                FileNodeData::GlobalIdTableEntryFNDX(entry) => {
                    id_map.add_mapping(entry.index, entry.guid);
                }
                FileNodeData::GlobalIdTableEntry2FNDX(entry) => {
                    // Inherit a GUID from the dependency revision's global identification table.
                    // See [MS-ONESTORE] 2.5.11.
                    match parent.and_then(|p| p.guid_for_index(entry.i_index_map_from)) {
                        Some(guid) => id_map.add_mapping(entry.i_index_map_to, guid),
                        None => {
                            return Err(onestore_parse_error!(
                                "GlobalIdTableEntry2FNDX references index {} that is not present \
                                 in the dependency revision's global ID table",
                                entry.i_index_map_from
                            )
                            .into());
                        }
                    }
                }
                FileNodeData::GlobalIdTableEntry3FNDX(_entry) => {
                    return Err(onestore_parse_error!(
                        "FileNodeData::GlobalIdTableEntry3FNDX has not been implemented yet",
                    )
                    .into());
                }
                FileNodeData::UnknownNode(_node) => {
                    log::warn!(
                        "Unknown node {:?} skipped while parsing global ID table.",
                        node
                    );
                }
                _ => {
                    return Err(onestore_parse_error!(
                        "Unexpected node ({:?}) encountered while parsing global ID table",
                        node
                    )
                    .into());
                }
            }
        }

        Ok(Self { id_map })
    }

    pub(crate) fn resolve_id(&self, id: &CompactId) -> Result<ExGuid> {
        self.id_map.resolve_id(id)
    }
}

impl Default for GlobalIdTable {
    fn default() -> Self {
        Self {
            id_map: IdMapping::new(),
        }
    }
}
