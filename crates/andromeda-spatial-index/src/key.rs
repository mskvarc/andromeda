//! How both indexes are partitioned.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
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
    /// The GeoProperty the partition indexes, named by its expanded IRI — for
    /// instance `https://uri.etsi.org/ngsi-ld/location`.
    pub geoproperty: AttributeName,
}

impl SpatialKey {
    /// The partition an entity of this type, in this tenant, is indexed under.
    pub const fn new(tenant: TenantRef, type_iri: IriBuf, geoproperty: AttributeName) -> SpatialKey {
        SpatialKey { tenant, type_iri, geoproperty }
    }

    /// The wildcard partition for a geo subscription that names no concrete
    /// type — an empty selector list, or a boolean type expression the per-type
    /// partitioning cannot enumerate.
    ///
    /// The subscription cache indexes such subscriptions here, and the
    /// notification worker queries this same partition, so an untyped or
    /// boolean-typed geo subscription is still reached for any entity change;
    /// the residual filter then re-checks the real selector. Both sides mint the
    /// key through this one constructor so the synthetic type IRI cannot drift.
    pub fn any_type(tenant: TenantRef, geoproperty: AttributeName) -> SpatialKey {
        SpatialKey::new(tenant, IriBuf::from(iri!("urn:andromeda:spatial:any-type")), geoproperty)
    }
}

/// The concrete type IRIs a clause 4.17 type selection reaches.
///
/// Returns `None` for an expression containing an `And`: an intersection over
/// multi-typed entities is not what the partitioning models, and answering
/// from the tree anyway would silently narrow the result. The caller falls
/// back to the store in that case.
pub fn flatten_or_type_iris(expr: &TypeExpr) -> Option<Vec<IriBuf>> {
    match expr {
        TypeExpr::Wildcard => None,
        // clone: the spatial key owns its IRIs and the expression is borrowed
        TypeExpr::Type(iri) => Some(vec![iri.clone()]),
        TypeExpr::Or(children) => {
            let mut out = Vec::with_capacity(children.len());
            for child in children {
                out.extend(flatten_or_type_iris(child)?);
            }
            Some(out)
        }
        TypeExpr::And(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::key::{SpatialKey, flatten_or_type_iris};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use iri_rs::IriBuf;

    fn type_expr(raw: &str) -> TypeExpr {
        TypeExpr::Type(IriBuf::new(raw.to_owned()).expect("a well-formed IRI"))
    }

    #[test]
    fn the_wildcard_partition_key_is_the_same_for_writer_and_reader() {
        // The cache writes under this key and the worker reads it; minting it
        // through one constructor is what guarantees they cannot drift.
        let geoproperty: AttributeName = "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name");
        // clone: both keys own their own copy, which is what the equality asserts
        let written = SpatialKey::any_type(TenantRef::UnnamedDefault, geoproperty.clone());
        let read = SpatialKey::any_type(TenantRef::UnnamedDefault, geoproperty);
        pretty_assertions::assert_eq!(written, read);
    }

    #[test]
    fn a_single_type_flattens_to_itself() {
        let flattened = flatten_or_type_iris(&type_expr("https://example.org/Vehicle")).expect("flattens");
        pretty_assertions::assert_eq!(flattened.len(), 1);
        pretty_assertions::assert_eq!(flattened[0].as_str(), "https://example.org/Vehicle");
    }

    #[test]
    fn an_alternation_flattens_to_every_branch() {
        let expr = TypeExpr::Or(vec![type_expr("https://example.org/Vehicle"), type_expr("https://example.org/Building")]);
        let flattened = flatten_or_type_iris(&expr).expect("flattens");
        pretty_assertions::assert_eq!(flattened.len(), 2);
    }

    #[test]
    fn an_intersection_refuses_to_flatten_rather_than_narrowing_the_answer() {
        let expr = TypeExpr::And(vec![type_expr("https://example.org/Vehicle"), type_expr("https://example.org/Building")]);
        assert!(flatten_or_type_iris(&expr).is_none());
    }

    #[test]
    fn an_intersection_nested_inside_an_alternation_refuses_too() {
        let nested = TypeExpr::And(vec![type_expr("https://example.org/A"), type_expr("https://example.org/B")]);
        let expr = TypeExpr::Or(vec![type_expr("https://example.org/Vehicle"), nested]);
        assert!(flatten_or_type_iris(&expr).is_none());
    }
}
