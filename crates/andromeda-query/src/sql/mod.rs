//! Cross-cutting SQL primitives shared by every query builder.
//!
//! Nothing here knows about an NGSI-LD concept: bind-value typing, the built
//! statement, schema identifiers, JSONB escaping and the two operators
//! sea-query has no native constructor for.

pub mod bind;
pub mod built;
pub mod jsonb;
pub mod literal;
pub mod predicate;
pub mod schema;
