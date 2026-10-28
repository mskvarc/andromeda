//! The stored Context Source Property document of a registration.
//!
//! Registration discovery evaluates a clause 4.9 `csf` against this one
//! column, and every registration write fills it, so a row without it is a row
//! no `csf` could ever read.

use crate::repository::{PinError, schema, written};

#[test]
fn every_registration_stores_its_context_source_property_document() -> Result<(), PinError> {
    let schema = schema()?;
    let column = schema
        .columns()
        .iter()
        .find(|column| column.relation() == "context_source_registrations" && column.name() == "context_source_properties")
        .ok_or_else(|| PinError::Missing("the Context Source Property document".to_owned()))?;
    pretty_assertions::assert_eq!(written(&schema, column.origin(), column.definition()), "jsonb compression lz4 not null");
    Ok(())
}
