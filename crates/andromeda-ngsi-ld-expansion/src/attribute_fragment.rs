//! The expanded attribute fragment of a Partial Attribute Update (clause
//! 5.6.4) or a Replace Attribute (clause 5.6.19).
//!
//! Both operations name the attribute in the request path and write one
//! instance of it in the body; expansion decides the attribute's IRI under the
//! request's context and reads the instance.

use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// One written attribute instance and the expanded name it is written under.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeFragment {
    /// The attribute, by its expanded IRI.
    name: AttributeName,
    /// The written instance.
    instance: InstanceDocument,
}

impl AttributeFragment {
    /// Pair an instance with the expanded name it is written under.
    #[must_use]
    pub const fn new(name: AttributeName, instance: InstanceDocument) -> AttributeFragment {
        AttributeFragment { name, instance }
    }

    /// The attribute, by its expanded IRI.
    #[must_use]
    pub const fn name(&self) -> &AttributeName {
        &self.name
    }

    /// The written instance.
    #[must_use]
    pub const fn instance(&self) -> &InstanceDocument {
        &self.instance
    }

    /// Give the fragment up as its parts.
    #[must_use]
    pub fn into_parts(self) -> (AttributeName, InstanceDocument) {
        (self.name, self.instance)
    }
}
