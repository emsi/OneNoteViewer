use crate::errors::Result;
use crate::fsshttpb::data::exguid::ExGuid;
use crate::onestore::desktop::file_node::FileNodeData;
use crate::onestore::desktop::file_structure::FileNodeDataIterator;
use crate::onestore::desktop::objects::global_id_table::GlobalIdTable;
use crate::onestore::desktop::objects::object::Object;
use crate::onestore::desktop::objects::parse_context::ParseContext;
use std::collections::HashMap;
use std::fmt::Debug;

/// See [MS-ONESTORE 2.1.13](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-onestore/607a84d4-5762-4a3e-9244-c91acddcf647)
pub(crate) struct ObjectGroupList {
    id: ExGuid,
    // TODO: Unused?
    // pub(crate) id_table: GlobalIdTable,
    pub(crate) objects: Vec<Object>,
}

impl Debug for ObjectGroupList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Don't include all fields -- avoid duplicate data in output
        f.debug_struct("ObjectGroupList")
            .field("id", &self.id)
            .field("objects", &self.objects)
            .finish()
    }
}

impl ObjectGroupList {
    pub(crate) fn try_parse_into<'a>(
        iterator: &mut FileNodeDataIterator<'a>,
        context: &'a ParseContext<'a>,
        objects: &mut HashMap<ExGuid, crate::onestore::Object>,
    ) -> Result<Option<()>> {
        let current = iterator.peek();
        if let Some(FileNodeData::ObjectGroupListReferenceFND(data)) = current {
            iterator.next();
            let mut list_iterator = data.list.iter_data();
            Self::parse_into(&mut list_iterator, context, objects)?;

            Ok(Some(()))
        } else if let Some(FileNodeData::ObjectGroupStartFND(_)) = current {
            Self::parse_into(iterator, context, objects)?;

            Ok(Some(()))
        } else {
            Ok(None)
        }
    }

    fn parse_into<'a>(
        iterator: &mut FileNodeDataIterator<'a>,
        context: &'a ParseContext<'a>,
        objects: &mut HashMap<ExGuid, crate::onestore::Object>,
    ) -> Result<()> {
        let _start = match iterator.next() {
            Some(FileNodeData::ObjectGroupStartFND(object)) => object,
            _ => {
                return Err(onestore_parse_error!(
                    "Object group lists must start with an ObjectGroupStartFND node."
                )
                .into());
            }
        };

        // Object groups only occur in `.one` files, whose global ID tables never use
        // `GlobalIdTableEntry2FNDX` dependency-revision references, so no parent table is needed.
        let id_table = GlobalIdTable::try_parse(iterator, None)?
            .ok_or_else(|| onestore_parse_error!("Global ID table not found in ObjectGroupList"))?;
        let parse_context = context.with_id_table(&id_table);

        let mut last_index = iterator.get_index();
        while let Some(item) = iterator.peek() {
            if matches!(item, FileNodeData::ObjectGroupEndFND) {
                break;
            } else if let FileNodeData::DataSignatureGroupDefinitionFND(_) = item {
                // Marks the end of a signature block. Ignored.
                // See https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-onestore/0fa4c886-011a-4c19-9651-9a69e43a19c6
                iterator.next();
            } else if let Some(object) = Object::try_parse(iterator, &parse_context)? {
                let id = id_table.resolve_id(&object.compact_id)?;
                objects.insert(id, object.data);
            } else {
                return Err(onestore_parse_error!(
                    "Unexpected node in ObjectGroupList: {:?}",
                    item
                )
                .into());
            }

            let index = iterator.get_index();
            if index == last_index {
                return Err(onestore_parse_error!(
                    "Parser did not advance while parsing ObjectGroupList entry: {:?}",
                    item
                )
                .into());
            }
            last_index = index;
        }

        Ok(())
    }
}
