use crate::errors::{Error, ErrorKind, Result};
use crate::one::property::PropertyType;
use crate::onestore::Object;
use time::{Duration, macros::utc_datetime};

/// A 32 bit date/time timestamp.
///
/// See [\[MS-ONE\] 2.3.1]
///
/// [\[MS-ONE\] 2.3.1]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/82336580-f956-40ea-94ab-d9ab15048395
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Time(u32);

impl Time {
    pub(crate) fn parse(prop_type: PropertyType, object: &Object) -> Result<Option<Time>> {
        let time = object
            .props
            .get(prop_type)
            .map(|value| {
                value.to_u32().ok_or_else(|| {
                    ErrorKind::MalformedOneNoteFileData("time value is not a u32".into())
                })
            })
            .transpose()?
            .map(Time);

        Ok(time)
    }
}

impl From<Time> for time::UtcDateTime {
    fn from(value: Time) -> Self {
        utc_datetime!(1980-01-01 0:00) + Duration::seconds(value.0 as i64)
    }
}

/// A 64 bit date/time timestamp.
///
/// See [\[MS-DTYP\] 2.3.3]
///
/// [\[MS-DTYP\] 2.3.3]: https://docs.microsoft.com/en-us/openspecs/windows_protocols/ms-dtyp/2c57429b-fdd4-488f-b5fc-9e4cf020fcdf
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct Timestamp(u64);

impl Timestamp {
    pub(crate) fn parse(prop_type: PropertyType, object: &Object) -> Result<Option<Timestamp>> {
        let timestamp = object
            .props
            .get(prop_type)
            .map(|value| {
                value.to_u64().ok_or_else(|| {
                    ErrorKind::MalformedOneNoteFileData("timestamp value is not a u64".into())
                })
            })
            .transpose()?
            .map(Timestamp);

        Ok(timestamp)
    }
}

impl TryFrom<Timestamp> for time::UtcDateTime {
    type Error = Error;

    fn try_from(value: Timestamp) -> Result<Self> {
        // FILETIME is 100-nanosecond intervals since 1601-01-01. Reduce to
        // milliseconds since `time` can't represent the full FILETIME range
        // and millisecond precision is sufficient for page timestamps.
        let microseconds = value.0 / 10;
        utc_datetime!(1601-01-01 0:00)
            .checked_add(Duration::milliseconds((microseconds / 1000) as i64))
            .ok_or_else(|| {
                ErrorKind::MalformedOneNoteFileData(
                    format!("timestamp out of range: {}", value.0).into(),
                )
                .into()
            })
    }
}
