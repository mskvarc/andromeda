//! The one clause 5.12 specification a batch's candidate registrations are
//! read with.
//!
//! The store answers a superset that the narrowing of each registration then
//! decides entity by entity, so one specification covers the whole array,
//! naming every identifier and every type an entity carries. A registration
//! matching one entity matches its identifier and one of its types, so it
//! matches the specification too. An entity whose types are not known, as in a
//! deletion, is matched on its identifier alone, so it leaves the types
//! unconstrained.

use crate::provision_batch::{BatchEntity, narrowing::entity_types};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{csr::expanded_entity_specification::ExpandedEntitySpecification, type_selection::grammar::TypeExpr};
use iri_rs::IriBuf;
use std::collections::BTreeSet;

/// Every identifier and every type of a batch's entities.
pub(super) struct BatchFootprint {
    /// The identifier of every entity.
    ids: Vec<Identifier>,
    /// Every type an entity carries, as one disjunction; `None` when an
    /// entity's types are not known, which leaves the types unconstrained.
    types: Option<TypeExpr>,
}

impl BatchFootprint {
    /// The footprint of `entities`.
    pub(super) fn of(entities: &[BatchEntity<'_>]) -> BatchFootprint {
        BatchFootprint {
            ids: entities.iter().map(|entity| entity.id.clone()).collect(), // clone: the store reads the identifiers as one slice
            types: type_union(entities),
        }
    }

    /// The specification the store preselects the batch's candidates with.
    pub(super) fn specification(&self) -> ExpandedEntitySpecification<'_> {
        ExpandedEntitySpecification {
            entity_type: self.types.as_ref(),
            ids: &self.ids,
            id_pattern: None,
        }
    }
}

/// Every type the entities carry, as one disjunction, or `None` when an entity
/// carries none that is known or one that is no IRI.
fn type_union(entities: &[BatchEntity<'_>]) -> Option<TypeExpr> {
    let mut named = BTreeSet::new();
    for entity in entities {
        let types = entity_types(entity);
        if types.is_empty() {
            return None;
        }
        named.extend(types);
    }
    let alternatives = named
        .into_iter()
        .map(|name| IriBuf::new(name.to_string()).ok().map(TypeExpr::Type))
        .collect::<Option<Vec<_>>>()?;
    Some(match <[TypeExpr; 1]>::try_from(alternatives) {
        Ok([single]) => single,
        Err(several) => TypeExpr::Or(several),
    })
}

#[cfg(test)]
mod tests {
    use crate::provision_batch::{
        BatchEntity,
        footprint::BatchFootprint,
        registration_fixture::{document, uri},
    };
    use andromeda_jsonld::{batch_contexts::BatchContext, context_spec::ContextSpec};
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    const VEHICLE: &str = "https://example.org/Vehicle";
    const BOAT: &str = "https://example.org/Boat";

    fn named(entity_type: &str) -> TypeExpr {
        TypeExpr::Type(IriBuf::new(entity_type.to_owned()).expect("a valid IRI"))
    }

    #[test]
    fn the_footprint_names_every_identifier_and_every_type_once() {
        let context = BatchContext::shared(ContextSpec::none());
        let ids = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Boat:1"), uri("urn:ngsi-ld:Vehicle:2")];
        let documents = [
            document("urn:ngsi-ld:Vehicle:1", VEHICLE, &[]),
            document("urn:ngsi-ld:Boat:1", BOAT, &[]),
            document("urn:ngsi-ld:Vehicle:2", VEHICLE, &[]),
        ];
        let entities: Vec<BatchEntity<'_>> = ids
            .iter()
            .zip(&documents)
            .map(|(id, document)| BatchEntity {
                id,
                document: Some(document),
                context: &context,
            })
            .collect();
        let footprint = BatchFootprint::of(&entities);
        let specification = footprint.specification();
        assert_eq!(specification.ids, ids.as_slice());
        assert_eq!(specification.entity_type, Some(&TypeExpr::Or(vec![named(BOAT), named(VEHICLE)])));
        assert!(specification.id_pattern.is_none());
    }

    #[test]
    fn one_type_is_named_alone() {
        let context = BatchContext::shared(ContextSpec::none());
        let id = uri("urn:ngsi-ld:Vehicle:1");
        let vehicle = document("urn:ngsi-ld:Vehicle:1", VEHICLE, &[]);
        let entities = [BatchEntity {
            id: &id,
            document: Some(&vehicle),
            context: &context,
        }];
        let footprint = BatchFootprint::of(&entities);
        assert_eq!(footprint.specification().entity_type, Some(&named(VEHICLE)));
    }

    /// Clause 5.12: an entity whose types are not known is matched on its
    /// identifier alone, so a deletion constrains no type.
    #[test]
    fn an_entity_without_known_types_leaves_the_types_unconstrained() {
        let context = BatchContext::shared(ContextSpec::none());
        let ids = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")];
        let vehicle = document("urn:ngsi-ld:Vehicle:1", VEHICLE, &[]);
        let entities = [
            BatchEntity {
                id: &ids[0],
                document: Some(&vehicle),
                context: &context,
            },
            BatchEntity {
                id: &ids[1],
                document: None,
                context: &context,
            },
        ];
        let footprint = BatchFootprint::of(&entities);
        assert!(footprint.specification().entity_type.is_none());
        assert_eq!(footprint.specification().ids, ids.as_slice());
    }
}
