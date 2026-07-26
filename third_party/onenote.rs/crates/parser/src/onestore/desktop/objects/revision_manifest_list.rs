use std::collections::{HashMap, HashSet};

use crate::errors::{ErrorKind, Result};
use crate::fsshttpb::data::exguid::ExGuid;
use crate::onestore::desktop::file_node::FileNodeData;
use crate::onestore::desktop::file_node::revision_manifest::RevisionManifestListStartFND;
use crate::onestore::desktop::file_structure::FileNodeDataIterator;
use crate::onestore::desktop::objects::id_mapping::IdMapping;
use crate::onestore::desktop::objects::parse_context::ParseContext;
use crate::onestore::desktop::objects::revision;

#[derive(Debug)]
pub(crate) struct RevisionManifestList {}

impl<'a> RevisionManifestList {
    pub(crate) fn try_parse_into(
        iterator: &mut FileNodeDataIterator<'a>,
        context: &'a ParseContext<'a>,
        roots: &mut HashMap<crate::onestore::desktop::objects::revision::RootRole, ExGuid>,
        objects: &mut HashMap<ExGuid, crate::onestore::Object>,
    ) -> Result<Option<()>> {
        let next = iterator.peek();

        match next {
            Some(FileNodeData::RevisionManifestListStartFND(list_reference)) => {
                iterator.next();
                Self::parse_into(iterator, list_reference, context, roots, objects)?;
                Ok(Some(()))
            }
            _ => Ok(None),
        }
    }

    fn parse_into(
        iterator: &mut FileNodeDataIterator<'a>,
        _list_reference: &RevisionManifestListStartFND,
        context: &'a ParseContext<'a>,
        roots: &mut HashMap<crate::onestore::desktop::objects::revision::RootRole, ExGuid>,
        objects: &mut HashMap<ExGuid, crate::onestore::Object>,
    ) -> Result<()> {
        let mut revisions_seen: HashSet<ExGuid> = HashSet::new();

        // Merged global ID table of each revision in this list, keyed by revision id. Lets a
        // dependent revision resolve `GlobalIdTableEntry2FNDX` references into the global ID table
        // of its dependency revision ([MS-ONESTORE] 2.5.11).
        let mut revision_id_maps: HashMap<ExGuid, IdMapping> = HashMap::new();

        let mut last_index = iterator.get_index();
        while let Some(current) = iterator.peek() {
            match current {
                FileNodeData::RevisionManifestEndFND => {
                    break;
                }
                FileNodeData::RevisionRoleDeclarationFND(_data) => {
                    // Ignore. If present, should always have revision_role = 0x1?
                    iterator.next();
                }
                FileNodeData::RevisionRoleAndContextDeclarationFND(data) => {
                    // Adds an additional (revision role, context) pair to some prior revision
                    // in the list.
                    // See [MS-ONESTORE 2.5.18](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-onestore/4863b0e8-fe14-49bb-a634-558c747bf0b8).
                    let base_rid: ExGuid = data.base.rid.into();
                    if revisions_seen.contains(&base_rid) {
                        iterator.next();
                        // Per MS-ONESTORE 2.1.12, revision_role *should* always be 0x1.
                        if data.base.revision_role != 0x1 {
                            // TODO: Find a test .one file that uses this and implement.
                            log::warn!(
                                "TO-DO: Apply the new role and context to the revision (role {:x})",
                                data.base.revision_role
                            );
                        }
                    } else {
                        return Err(
                            ErrorKind::MalformedOneStoreData("RevisionRoleAndContextDeclarationFND points to a non-existent revision".into()).into()
                        );
                    }
                }
                node => {
                    let revision_id = revision::try_parse_into(
                        iterator,
                        context,
                        roots,
                        objects,
                        &mut revision_id_maps,
                    )?
                    .ok_or_else(|| {
                        onestore_parse_error!(
                            "Unexpected node encountered in RevisionManifestList: {:?}",
                            node
                        )
                    })?;
                    revisions_seen.insert(revision_id);
                }
            }

            let index = iterator.get_index();
            if index == last_index {
                return Err(onestore_parse_error!(
                    "Parser did not advance while parsing RevisionManifestList entry: {:?}",
                    current
                )
                .into());
            }
            last_index = index;
        }

        Ok(())
    }
}
