//! The operation codes an Entity Map's request record may store.

use crate::repository::{PinError, constraint_definition, schema};
use andromeda_postgresql_stored_values::entity_map_request_kind::EntityMapRequestKindCode;
use pretty_assertions::assert_eq;

/// The check admits exactly the codes the registry assigns, so an operation
/// added to the registry without a migration widening the check fails here
/// rather than as a constraint violation on the first map it creates.
#[test]
fn the_request_kind_check_admits_every_registered_operation_code() -> Result<(), PinError> {
    let schema = schema()?;
    let codes: Vec<i16> = EntityMapRequestKindCode::ALL.iter().map(|kind| kind.code()).collect();
    let (Some(lowest), Some(highest)) = (codes.iter().min(), codes.iter().max()) else {
        return Err(PinError::Missing("a registered operation code".to_owned()));
    };
    assert_eq!(codes, (*lowest..=*highest).collect::<Vec<_>>(), "the codes are contiguous");
    assert_eq!(
        constraint_definition(&schema, "chk_entity_maps_request_kind")?,
        format!("check (request_kind between {lowest} and {highest})")
    );
    Ok(())
}
