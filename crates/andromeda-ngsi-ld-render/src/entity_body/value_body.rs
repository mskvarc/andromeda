//! A Property value written as the client's own JSON.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_model::attribute::value::NgsiLdValue;
use andromeda_ngsi_ld_representation::concise_reading::{bare_value_family, typeless_value_family};
use serde::{Serialize, Serializer};

/// One clause 4.6.3 value: the client's JSON as it was written.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueBody<'a>(pub(crate) &'a NgsiLdValue);

impl ValueBody<'_> {
    /// The family the value, written bare in an attribute's place, reads back
    /// as the value of (clause 4.5.2.3), or `None` when it reads as something
    /// else.
    pub(crate) fn bare_family(self) -> Option<AttributeType> {
        bare_value_family(self.0.literal().piece())
    }

    /// The family a typeless instance carrying the value under `value` reads
    /// back as (clause 4.5.2.3, "type").
    pub(crate) fn typeless_family(self) -> AttributeType {
        typeless_value_family(self.0.literal().piece())
    }
}

impl Serialize for ValueBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// An ordered list of values, as a `ListProperty` holds them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueListBody<'a>(pub(crate) &'a [NgsiLdValue]);

impl Serialize for ValueListBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(ValueBody))
    }
}
