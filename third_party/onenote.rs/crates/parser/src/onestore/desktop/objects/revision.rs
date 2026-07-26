//! Parses a OneStore revision.
//!
//! See [MS-ONESTORE 2.1.9](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-onestore/90101e91-2f7f-4753-9332-31bed5b5c49d).

use super::object_group_list::ObjectGroupList;
use crate::errors::{Error, Result};
use crate::fsshttpb::data::exguid::ExGuid;
use crate::onestore::desktop::file_node::FileNodeData;
use crate::onestore::desktop::file_structure::FileNodeDataIterator;
use crate::onestore::desktop::objects::global_id_table::GlobalIdTable;
use crate::onestore::desktop::objects::id_mapping::IdMapping;
use crate::onestore::desktop::objects::object::Object;
use crate::onestore::desktop::objects::parse_context::ParseContext;
use crate::onestore::shared::compact_id::CompactId;
use crate::onestore::shared::object_prop_set::ObjectPropSet;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::convert::TryInto;

// See [MS-ONE 2.1.8](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-one/037e31c0-4484-4a14-819a-0ddece2cacbc)
#[derive(Eq, PartialEq, Hash, Debug, Copy, Clone)]
pub(crate) enum RootRole {
    DefaultContent,
    MetadataRoot,
    VersionMetadataRoot,
}

impl TryFrom<u32> for RootRole {
    type Error = Error;
    fn try_from(value: u32) -> std::result::Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::DefaultContent),
            2 => Ok(Self::MetadataRoot),
            4 => Ok(Self::VersionMetadataRoot),
            other => Err(onestore_parse_error!("Invalid root role: {}", other).into()),
        }
    }
}

pub(crate) fn try_parse_into<'a>(
    iterator: &mut FileNodeDataIterator<'a>,
    context: &'a ParseContext<'a>,
    roots: &mut HashMap<RootRole, ExGuid>,
    objects: &mut HashMap<ExGuid, crate::onestore::Object>,
    revision_id_maps: &mut HashMap<ExGuid, IdMapping>,
) -> Result<Option<ExGuid>> {
    let next = iterator.peek();

    match next {
        Some(
            FileNodeData::RevisionManifestStart4FND(_)
            | FileNodeData::RevisionManifestStart6FND(_)
            | FileNodeData::RevisionManifestStart7FND(_),
        ) => Ok(Some(parse_into(
            iterator,
            context,
            roots,
            objects,
            revision_id_maps,
        )?)),
        _ => Ok(None),
    }
}

fn parse_into<'a>(
    iterator: &mut FileNodeDataIterator<'a>,
    context: &'a ParseContext<'a>,
    roots: &mut HashMap<RootRole, ExGuid>,
    objects: &mut HashMap<ExGuid, crate::onestore::Object>,
    revision_id_maps: &mut HashMap<ExGuid, IdMapping>,
) -> Result<ExGuid> {
    macro_rules! iterator_skip_if_matching {
        ($iterator:expr, $match_condition:pat) => {
            if matches!($iterator.peek(), $match_condition) {
                $iterator.next();
            }
        };
    }

    let start = iterator.next();
    let (id, parent_id): (ExGuid, ExGuid) = match start {
        Some(FileNodeData::RevisionManifestStart4FND(data)) => {
            (data.rid.into(), data.rid_dependent.into())
        }
        Some(FileNodeData::RevisionManifestStart6FND(data)) => {
            (data.rid.into(), data.rid_dependent.into())
        }
        Some(FileNodeData::RevisionManifestStart7FND(data)) => {
            (data.base.rid.into(), data.base.rid_dependent.into())
        }
        _ => {
            return Err(
                onestore_parse_error!("Invalid start node for revision: {:?}", start).into(),
            );
        }
    };

    // The global ID table of the revision this one depends on. `ridDependent` always refers to an
    // earlier revision manifest in the list ([MS-ONESTORE] 2.5.6), so its merged mapping has
    // already been recorded. It is needed to resolve `GlobalIdTableEntry2FNDX` references.
    let parent_id_map = revision_id_maps.get(&parent_id).cloned();

    // Accumulates the entries of every global ID table in this revision so that a later dependent
    // revision can inherit from them.
    let mut revision_id_map = IdMapping::new();

    let mut global_id_tables: Vec<GlobalIdTable> = Vec::new();

    let mut last_index = iterator.get_index();
    while let Some(current) = iterator.peek() {
        if let FileNodeData::RevisionManifestEndFND = current {
            iterator.next();
            break;
        } else if let Some(()) = ObjectGroupList::try_parse_into(iterator, context, objects)? {
            // Skip: Used for reference counting (which we can ignore here)
            iterator_skip_if_matching!(
                iterator,
                Some(FileNodeData::ObjectInfoDependencyOverridesFND(_))
            );
        } else if let Some(global_id_table) =
            GlobalIdTable::try_parse(iterator, parent_id_map.as_ref())?
        {
            revision_id_map.merge(&global_id_table.id_map);

            // In .onetoc2 files, objects can directly follow GlobalIdTables:
            let parse_context = context.with_id_table(&global_id_table);
            iterator_skip_if_matching!(
                iterator,
                Some(FileNodeData::DataSignatureGroupDefinitionFND(_))
            );

            loop {
                if let Some(object) = Object::try_parse(iterator, &parse_context)? {
                    let id = global_id_table.resolve_id(&object.compact_id)?;
                    objects.insert(id, object.data);
                } else {
                    // Object revisions ([MS-ONESTORE] 2.5.13/2.5.14) revise an
                    // object that was declared by an earlier revision. They carry
                    // a new property set but not a JCID, since the object's type
                    // MUST NOT change when it is revised ([MS-ONESTORE] 2.1.5).
                    let revision = match iterator.peek() {
                        Some(FileNodeData::ObjectRevisionWithRefCountFNDX(rev)) => {
                            Some((rev.oid(), rev.property_set().clone()))
                        }
                        Some(FileNodeData::ObjectRevisionWithRefCount2FNDX(rev)) => {
                            Some((rev.oid(), rev.property_set().clone()))
                        }
                        _ => None,
                    };

                    match revision {
                        Some((oid, props)) => {
                            iterator.next();
                            apply_object_revision(
                                &oid,
                                props,
                                &global_id_table,
                                &parse_context,
                                objects,
                            )?;
                        }
                        None => break,
                    }
                }

                // Skip the reference counting object, if present
                iterator_skip_if_matching!(
                    iterator,
                    Some(FileNodeData::ObjectInfoDependencyOverridesFND(_))
                );
            }

            global_id_tables.push(global_id_table);
        } else if let FileNodeData::RootObjectReference3FND(object_reference) = current {
            iterator.next(); // Consume the reference

            let root_role: RootRole = object_reference.root_role.try_into()?;
            if roots.contains_key(&root_role) {
                log::warn!("An item with role {:?} is already present", root_role);
            }

            roots
                .entry(root_role)
                .or_insert(object_reference.oid_root.into());
        } else if let FileNodeData::RootObjectReference2FNDX(object_reference) = current {
            // .onetoc2
            iterator.next();
            let oid_root = global_id_tables
                .last()
                .ok_or_else(|| {
                    onestore_parse_error!(
                        "Unable to resolve RootObjectReference2FNDX ID: no global ID table found"
                    )
                })?
                .resolve_id(&object_reference.oid_root)?;
            roots
                .entry(object_reference.root_role.try_into()?)
                .or_insert(oid_root);
        } else if let FileNodeData::DataSignatureGroupDefinitionFND(_) = current {
            // Marks the end of a signature block (.onetoc2). Ignored.
            // See https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-onestore/0fa4c886-011a-4c19-9651-9a69e43a19c6
            iterator.next();
        } else {
            return Err(
                onestore_parse_error!("Unexpected node (parsing Revision): {:?}", current).into(),
            );
        }

        // Prevent infinite loops
        let current_index = iterator.get_index();
        if current_index == last_index {
            return Err(onestore_parse_error!(
                "Parser did not advance while parsing Revision entry: {:?}",
                current
            )
            .into());
        }
        last_index = current_index;
    }

    revision_id_maps.insert(id, revision_id_map);

    Ok(id)
}

/// Applies an object revision ([MS-ONESTORE] 2.5.13/2.5.14) to the object map.
///
/// A revision node supplies a fresh property set for an object that was declared
/// by an earlier revision but does not restate the object's JCID, because the
/// type of an object MUST NOT change when it is revised ([MS-ONESTORE] 2.1.5).
/// The dependency (declaring) revision always precedes the revising one in the
/// file, since `RevisionManifestStart*.ridDependent` points to an earlier
/// revision manifest, so the original declaration's JCID is already known.
fn apply_object_revision(
    oid: &CompactId,
    props: ObjectPropSet,
    global_id_table: &GlobalIdTable,
    parse_context: &ParseContext,
    objects: &mut HashMap<ExGuid, crate::onestore::Object>,
) -> Result<()> {
    let id = global_id_table.resolve_id(oid)?;

    let Some(jc_id) = objects.get(&id).map(|object| object.jc_id) else {
        log::warn!("Object revision {id:?} has no prior declaration; skipping");
        return Ok(());
    };

    objects.insert(
        id,
        crate::onestore::Object {
            context_id: parse_context.context_id,
            jc_id,
            props,
            file_data: None,
            mapping: parse_context.id_map.clone(),
        },
    );

    Ok(())
}
