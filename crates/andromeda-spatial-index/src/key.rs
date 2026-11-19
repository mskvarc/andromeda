//! How both indexes are partitioned.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{name_stage::Expanded, type_selection::TypeSelection};
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::{IriBuf, iri};

/// Partition key for both spatial indexes.
///
/// Trees are partitioned by `(tenant, entity type, geoproperty)` so each
/// R-tree stays small and selective. The tenant component isolates
/// cross-tenant data; the type and geoproperty components mirror the
/// selectivity the subscription cache already gets from its type index.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SpatialKey {
    /// Owning tenant.
    pub tenant: TenantRef,
    /// Expanded entity type IRI.
    pub type_iri: IriBuf,
    /// The `GeoProperty` the partition indexes, named by its expanded IRI, for
    /// instance `https://uri.etsi.org/ngsi-ld/location`.
    pub geoproperty: AttributeName,
}

impl SpatialKey {
    /// The partition an entity of this type, in this tenant, is indexed under.
    #[must_use]
    pub const fn new(tenant: TenantRef, type_iri: IriBuf, geoproperty: AttributeName) -> SpatialKey {
        SpatialKey { tenant, type_iri, geoproperty }
    }

    /// The wildcard partition for a geo subscription that names no concrete
    /// type (an empty selector list, or a boolean type selection the per-type
    /// partitioning cannot enumerate).
    ///
    /// The subscription cache indexes such subscriptions here, and the
    /// notification worker queries this same partition, so an untyped or
    /// boolean-typed geo subscription is still reached for any entity change;
    /// the residual filter then re-checks the real selector. Both sides mint the
    /// key through this one constructor so the synthetic type IRI cannot drift.
    #[must_use]
    pub fn any_type(tenant: TenantRef, geoproperty: AttributeName) -> SpatialKey {
        SpatialKey::new(tenant, IriBuf::from(iri!("urn:andromeda:spatial:any-type")), geoproperty)
    }
}

/// The concrete type IRIs a clause 4.17 type selection reaches.
///
/// Returns `None` for the wildcard and for a selection with a conjunction of
/// several types: an intersection over multi-typed entities is not what the
/// partitioning models, and answering from the tree anyway would silently
/// narrow the result. The caller falls back to the store in that case.
#[must_use]
pub fn flatten_or_type_iris(expr: &TypeSelection<Expanded>) -> Option<Vec<IriBuf>> {
    let TypeSelection::Alternatives(alternatives) = expr else {
        return None;
    };
    alternatives
        .iter()
        .map(|conjunction| match conjunction {
            [only] => Some(only.as_iri().clone()),
            [] | [_, _, ..] => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::key::{SpatialKey, flatten_or_type_iris};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_iri::EntityTypeIri};
    use andromeda_ngsi_ld_query::{name_stage::Expanded, type_selection::TypeSelection};
    use andromeda_tenancy::tenant_ref::TenantRef;

    fn type_selection(raw: &str) -> TypeSelection<Expanded> {
        TypeSelection::single(raw.parse::<EntityTypeIri>().expect("a well-formed IRI"))
    }

    fn alternatives(shape: &[&[&str]]) -> TypeSelection<Expanded> {
        let conjunction = |types: &[&str]| types.iter().map(|raw| raw.parse::<EntityTypeIri>().expect("a well-formed IRI")).collect();
        TypeSelection::of_alternatives(shape.iter().map(|types| conjunction(types)).collect()).expect("a non-empty shape")
    }

    #[test]
    fn the_wildcard_partition_key_is_the_same_for_writer_and_reader() {
        // The cache writes under this key and the worker reads it; minting it
        // through one constructor is what guarantees they cannot drift.
        let geoproperty: AttributeName = "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name");
        let written = SpatialKey::any_type(TenantRef::UnnamedDefault, geoproperty.clone());
        let read = SpatialKey::any_type(TenantRef::UnnamedDefault, geoproperty);
        pretty_assertions::assert_eq!(written, read);
    }

    #[test]
    fn a_single_type_flattens_to_itself() {
        let flattened = flatten_or_type_iris(&type_selection("https://example.org/Vehicle")).expect("flattens");
        pretty_assertions::assert_eq!(flattened.len(), 1);
        pretty_assertions::assert_eq!(flattened[0].as_str(), "https://example.org/Vehicle");
    }

    #[test]
    fn an_alternation_flattens_to_every_branch() {
        let expr = alternatives(&[&["https://example.org/Vehicle"], &["https://example.org/Building"]]);
        let flattened = flatten_or_type_iris(&expr).expect("flattens");
        pretty_assertions::assert_eq!(flattened.len(), 2);
    }

    #[test]
    fn an_intersection_refuses_to_flatten_rather_than_narrowing_the_answer() {
        let expr = alternatives(&[&["https://example.org/Vehicle", "https://example.org/Building"]]);
        assert!(flatten_or_type_iris(&expr).is_none());
    }

    #[test]
    fn an_intersection_beside_an_alternative_refuses_too() {
        let expr = alternatives(&[&["https://example.org/Vehicle"], &["https://example.org/A", "https://example.org/B"]]);
        assert!(flatten_or_type_iris(&expr).is_none());
    }

    #[test]
    fn the_wildcard_refuses_to_flatten() {
        assert!(flatten_or_type_iris(&TypeSelection::Wildcard).is_none());
    }
}
