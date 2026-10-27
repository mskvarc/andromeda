//! The `jsonldContexts/{contextId}` path segment.

use andromeda_context_registry::identifier::ContextIdentifier;
use andromeda_http_extraction::path_segment::{PathParameter, PathSegment, SegmentFault};

/// The stored `@context` a `jsonldContexts/{contextId}` resource addresses.
///
/// Every segment is admitted: one that keys no stored entry is answered with
/// `ResourceNotFound` by the lookup (clauses 5.13.4.4 and 5.13.5.4), never
/// refused as malformed.
#[derive(Debug)]
pub(crate) struct ContextId(pub(crate) ContextIdentifier);

impl PathSegment for ContextId {
    const PARAMETER: PathParameter = PathParameter::ContextId;

    fn parse(segment: &str) -> Result<ContextId, SegmentFault> {
        Ok(ContextId(ContextIdentifier::from(segment)))
    }
}
