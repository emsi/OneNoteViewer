use crate::errors::Result;
use crate::fsshttpb::data::exguid::ExGuid;
use crate::one::property::object_reference::ObjectReference;
use crate::one::property::{PropertyType, simple};
use crate::one::property_set::{PropertySetId, assert_property_set};
use crate::onenote::ParserContext;
use crate::onestore::Object;

/// An ink data container.
#[allow(dead_code)]
pub(crate) struct Data {
    pub(crate) strokes: Vec<ExGuid>,
    pub(crate) bounding_box: Option<[i32; 4]>,
}

pub(crate) fn parse(object: &Object, ctx: &mut ParserContext) -> Result<Data> {
    assert_property_set(object, PropertySetId::InkDataNode)?;

    let strokes =
        ObjectReference::parse_vec(PropertyType::InkStrokes, object)?.unwrap_or_else(|| {
            // An InkDataNode can have no associated InkStrokes object.
            // See https://discourse.joplinapp.org/t/error-importing-notes-from-onenote/49671
            warn!(ctx, "ink data node {:?} has no strokes", object.id());

            vec![]
        });
    let bounding_box = simple::parse_vec_i32(PropertyType::InkBoundingBox, object)?
        .filter(|values| values.len() == 4)
        .map(|values| [values[0], values[1], values[2], values[3]]);

    Ok(Data {
        strokes,
        bounding_box,
    })
}
