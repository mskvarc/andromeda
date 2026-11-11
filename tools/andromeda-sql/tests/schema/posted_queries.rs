//! The API codes a posted query may store.

use crate::repository::{PinError, constraint_definition, schema};
use andromeda_postgresql_codes::posted_query_kind::PostedQueryKindCode;
use pretty_assertions::assert_eq;

/// The check admits exactly the codes the registry assigns, so an API added to
/// the registry without a migration widening the check fails here rather than
/// as a constraint violation on the first query posted to it.
#[test]
fn the_kind_check_admits_every_registered_api_code() -> Result<(), PinError> {
    let schema = schema()?;
    let codes: Vec<i16> = PostedQueryKindCode::ALL.iter().map(|kind| kind.code()).collect();
    let (Some(lowest), Some(highest)) = (codes.iter().min(), codes.iter().max()) else {
        return Err(PinError::Missing("a registered API code".to_owned()));
    };
    assert_eq!(codes, (*lowest..=*highest).collect::<Vec<_>>(), "the codes are contiguous");
    assert_eq!(
        constraint_definition(&schema, "chk_posted_queries_kind")?,
        format!("check (kind between {lowest} and {highest})")
    );
    Ok(())
}
