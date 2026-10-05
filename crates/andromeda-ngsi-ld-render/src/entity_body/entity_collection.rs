//! An array of entities, each carrying the body's `@context`.
//!
//! An array of entities is no JSON-LD document, so there is no enclosing
//! object the member could go on instead (clause 6.3.6): a query response
//! written in `application/ld+json` and the `data` of a notification written
//! in it both give every entity the one context.

use crate::entity_body::typed_entity::TypedEntity;
use andromeda_json_literal::JsonLiteral;
use serde::{Serialize, Serializer};

/// A collection of entities, each carrying the body's `@context`.
pub(crate) struct EntityCollection<'a> {
    /// The entities.
    pub(crate) entities: &'a [TypedEntity<'a>],
    /// The `@context` each carries, if the body carries one.
    pub(crate) context: Option<&'a JsonLiteral>,
}

impl Serialize for EntityCollection<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.entities.iter().map(|entity| TypedEntity {
            context: self.context,
            ..*entity
        }))
    }
}
