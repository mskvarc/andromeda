//! How both indexes are partitioned.

use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::IriBuf;

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
    /// Expanded geoproperty IRI, such as `https://uri.etsi.org/ngsi-ld/location`.
    pub geoproperty_iri: IriBuf,
}

impl SpatialKey {
    /// The partition an entity of this type, in this tenant, is indexed under.
    pub const fn new(tenant: TenantRef, type_iri: IriBuf, geoproperty_iri: IriBuf) -> SpatialKey {
        SpatialKey {
            tenant,
            type_iri,
            geoproperty_iri,
        }
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
    use crate::key::flatten_or_type_iris;
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
    use iri_rs::IriBuf;

    fn type_expr(raw: &str) -> TypeExpr {
        TypeExpr::Type(IriBuf::new(raw.to_owned()).expect("a well-formed IRI"))
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
