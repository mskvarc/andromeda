//! The canonical identity of a compiled query.
//!
//! A query is the same query however it was written: `q` terms in another
//! order, `type=A|B` and `type=B|A`, `attrs` listed in another order. Every
//! consumer that asks whether two requests ask the same thing (a cursor walk that holds a followed pointer to the query it began with, an
//! Entity Map that holds a follow-up to the request that created it) reads
//! that sameness off one record built here, from the normal form of each
//! filter and the expanded selectors, so none of them re-decides what makes
//! two spellings one query.

use crate::{
    entity_selection::{EntitySelection, EntitySelector},
    filter::{compiled_filter::CompiledFilter, reading::filter_reading::FilterReading},
    geo::query::GeoQuery,
    pattern::Pattern,
    projection::projected_members::{ProjectedMembers, ScopeRetention},
    scope::query::ScopeQuery,
    temporal::compiled::CompiledTemporalQuery,
    type_selection::grammar::TypeExpr,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use iri_rs::IriBuf;
use serde::Serialize;
use thiserror::Error;

/// Whether the entity identifiers a query names are part of its identity.
///
/// Clause 5.5.14 holds every follow-up under an Entity Map to "the same
/// parameters as in the original request that created the `EntityMap`, except
/// for the specification of Entity identifiers or parameters related to
/// pagination", so an Entity Map's identity leaves the identifiers and the
/// identifier pattern out, while a cursor walk, which pages one result that
/// the identifiers decide, keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierScope {
    /// The identifiers and the identifier pattern of every selector are part
    /// of the identity.
    Kept,
    /// The identity holds only the clause 4.17 type alternatives of the
    /// selectors, which is what clause 5.5.14 leaves once the specification
    /// of Entity identifiers is excepted.
    Excepted,
}

/// The identity of a query could not be written as bytes.
#[derive(Debug, Error)]
pub enum QueryIdentityError {
    /// A member of the identity has no serialised form.
    #[error("the query identity has no canonical form")]
    Unwritable(#[from] serde_json::Error),
}

/// What a compiled query selects, borrowed from its compiled form.
///
/// Presentation and paging are not part of it: they decide how a result is
/// shown or which part of it a request reads. Nor is the temporal window,
/// which clause 5.7.4.4 step S1 does read to select Entities: clause 5.5.14
/// excepts the temporal query from a follow-up under an Entity Map, and the
/// cursor digest carries the window beside this record. The serialised form of
/// this record, its derived
/// `Serialize`, is the canonical one, so two queries have one identity exactly
/// when [`QueryIdentity::record`] gives the same bytes.
#[derive(Debug, Serialize)]
pub struct QueryIdentity<'a> {
    /// What the selectors fix about the entities.
    entities: EntityAxes<'a>,
    /// The expanded attribute IRIs, sorted and deduplicated.
    attrs: Vec<&'a IriBuf>,
    /// The `q` filter in normal form.
    q: Option<&'a FilterReading>,
    /// The geo-query, parsed and validated.
    geo: Option<&'a GeoQuery>,
    /// The scope query.
    scope_q: Option<&'a ScopeQuery>,
    /// The context source filter in normal form.
    csf: Option<&'a FilterReading>,
    /// What an Entity has to carry for the `pick` or `omit` to leave it a
    /// member: an Entity they would empty is not selected, so they decide
    /// which Entities a query holds.
    members: MembersIdentity<'a>,
}

/// What an Entity has to carry for a projection to leave it a member, its
/// Attribute names sorted and deduplicated.
#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MembersIdentity<'a> {
    /// Every Entity keeps a member.
    Unconditional,
    /// One of these Attributes, or a `scope` the `pick` keeps.
    AnyOf {
        /// The Attributes the `pick` keeps.
        attributes: Vec<&'a IriBuf>,
        /// Whether the `pick` keeps `scope`.
        scope: ScopeRetention,
    },
    /// An Attribute besides these, or a `scope` the `omit` spares.
    AnyBesides {
        /// The Attributes the `omit` removes.
        attributes: Vec<&'a IriBuf>,
        /// Whether the `omit` spares `scope`.
        scope: ScopeRetention,
    },
}

impl<'a> MembersIdentity<'a> {
    /// The identity of `members`.
    fn of(members: &'a ProjectedMembers) -> MembersIdentity<'a> {
        match members {
            ProjectedMembers::Unconditional => MembersIdentity::Unconditional,
            ProjectedMembers::AnyOf { attributes, scope } => MembersIdentity::AnyOf {
                attributes: sorted_attrs(attributes),
                scope: *scope,
            },
            ProjectedMembers::AnyBesides { attributes, scope } => MembersIdentity::AnyBesides {
                attributes: sorted_attrs(attributes),
                scope: *scope,
            },
        }
    }
}

/// What the entity selectors of a query fix, by the scope of its identifiers.
#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum EntityAxes<'a> {
    /// Every selector, sorted and deduplicated: a union of selectors is
    /// independent of the order its members are written in (clause 5.2.33).
    Kept(Vec<SelectorIdentity<'a>>),
    /// The type alternatives of every selector, sorted and deduplicated.
    Excepted(Vec<&'a TypeExpr>),
}

/// One entity selector, its identifiers and type alternatives sorted.
#[derive(Debug, Serialize)]
struct SelectorIdentity<'a> {
    /// The identifiers the selector admits, sorted and deduplicated.
    ids: Vec<&'a Identifier>,
    /// The identifier pattern, absent when identifiers are named: clause 5.2.33
    /// gives `id` precedence over `idPattern`.
    id_pattern: Option<&'a Pattern>,
    /// The clause 4.17 alternatives of the expanded type expression, sorted and
    /// deduplicated, so `type=A|B` and `type=B|A` select alike; empty when the
    /// selector names no type.
    types: Vec<&'a TypeExpr>,
}

/// The order selectors are sorted in, which orders identifiers by their text.
type SelectorKey<'a> = (Vec<&'a str>, Option<&'a Pattern>, Vec<&'a TypeExpr>);

impl<'a> SelectorIdentity<'a> {
    /// The identity of `selector`.
    fn of(selector: &'a EntitySelector) -> SelectorIdentity<'a> {
        let mut ids: Vec<&Identifier> = selector.ids.iter().collect();
        ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        ids.dedup();
        let ids = ids;
        SelectorIdentity {
            id_pattern: selector.id_pattern.as_ref().filter(|_| ids.is_empty()),
            ids,
            types: type_alternatives(selector.type_expr.iter()),
        }
    }

    /// The key selectors are sorted and deduplicated by.
    fn key(&self) -> SelectorKey<'a> {
        (self.ids.iter().copied().map(Identifier::as_str).collect(), self.id_pattern, self.types.clone())
    }
}

impl<'a> EntityAxes<'a> {
    /// The axes the selectors of `selection` fix under `scope`.
    ///
    /// A union containing an unconstrained selector places no constraint, so
    /// its identity holds no selector at all.
    fn of(selection: &'a EntitySelection, scope: IdentifierScope) -> EntityAxes<'a> {
        let selectors = selection.selectors.as_slice();
        match scope {
            IdentifierScope::Kept => {
                let constrained = selection.selectors_constrain();
                let mut kept: Vec<SelectorIdentity<'a>> = selectors.iter().filter(|_| constrained).map(SelectorIdentity::of).collect();
                kept.sort_by_cached_key(SelectorIdentity::key);
                kept.dedup_by(|left, right| left.key() == right.key());
                EntityAxes::Kept(kept)
            }
            IdentifierScope::Excepted => EntityAxes::Excepted(type_alternatives(selectors.iter().filter_map(|selector| selector.type_expr.as_ref()))),
        }
    }
}

/// The clause 4.17 alternatives the type expressions are the union of, sorted
/// and deduplicated.
///
/// The selectors of a POST body (clause 5.2.33) are alternatives of one
/// another, as the operands of a top-level `|` are, so `type=A|B` and a body of
/// the selectors `A` and `B` select alike.
fn type_alternatives<'a>(expressions: impl Iterator<Item = &'a TypeExpr>) -> Vec<&'a TypeExpr> {
    let mut alternatives: Vec<&TypeExpr> = expressions.flat_map(TypeExpr::alternatives).collect();
    alternatives.sort_unstable();
    alternatives.dedup();
    alternatives
}

/// The expanded attribute IRIs, sorted and deduplicated.
fn sorted_attrs(attrs: &[IriBuf]) -> Vec<&IriBuf> {
    let mut attrs: Vec<&IriBuf> = attrs.iter().collect();
    attrs.sort_unstable();
    attrs.dedup();
    attrs
}

impl<'a> QueryIdentity<'a> {
    /// The identity of the query `selection` compiles, whose context source
    /// filter reads as `csf`, with the identifiers of `scope`.
    #[must_use]
    pub fn of(selection: &'a EntitySelection, csf: Option<&'a FilterReading>, scope: IdentifierScope) -> QueryIdentity<'a> {
        QueryIdentity {
            entities: EntityAxes::of(selection, scope),
            attrs: sorted_attrs(&selection.attrs),
            q: selection.q.as_ref().map(CompiledFilter::reading),
            geo: selection.geo.as_ref(),
            scope_q: selection.scope_q.as_ref(),
            csf,
            members: MembersIdentity::of(&selection.projected_members),
        }
    }

    /// The identity of the temporal query `query` compiles, whose context
    /// source filter reads as `csf`, with the identifiers of `scope`.
    ///
    /// A temporal query narrows the Attributes it represents by its `attrs`
    /// (clause 5.7.4.4), which it holds with its instance selection rather than
    /// with the entities it selects, so the identity takes them from there.
    #[must_use]
    pub fn of_temporal(query: &'a CompiledTemporalQuery, csf: Option<&'a FilterReading>, scope: IdentifierScope) -> QueryIdentity<'a> {
        QueryIdentity {
            attrs: sorted_attrs(&query.instances.attr_filter),
            ..QueryIdentity::of(&query.entities, csf, scope)
        }
    }

    /// The identity of a retrieval of one entity, which a type selector and an
    /// attribute list alone select (clauses 5.7.1.3 and 5.7.3.3).
    #[must_use]
    pub fn of_retrieval(entity_type: Option<&'a TypeExpr>, attrs: &'a [IriBuf]) -> QueryIdentity<'a> {
        QueryIdentity {
            entities: EntityAxes::Excepted(type_alternatives(entity_type.into_iter())),
            attrs: sorted_attrs(attrs),
            q: None,
            geo: None,
            scope_q: None,
            csf: None,
            members: MembersIdentity::Unconditional,
        }
    }

    /// The canonical bytes of the identity.
    ///
    /// # Errors
    ///
    /// [`QueryIdentityError::Unwritable`] when a member has no serialised form.
    pub fn record(&self) -> Result<Vec<u8>, QueryIdentityError> {
        Ok(serde_json::to_vec(self)?)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_selection::{EntitySelection, EntitySelector},
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse, reading::filter_reading::FilterReading},
        geo::query::GeoQuery,
        pattern::Pattern,
        projection::projected_members::{ProjectedMembers, ScopeRetention},
        query_identity::{IdentifierScope, QueryIdentity},
        scope::query::ScopeQuery,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_architecture_checks::assert_named_snapshot;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use iri_rs::IriBuf;
    use pretty_assertions::{assert_eq, assert_ne};

    fn iri(name: &str) -> IriBuf {
        format!("https://example.com/{name}").parse().expect("a valid IRI")
    }

    fn id(number: u32) -> Identifier {
        format!("urn:ngsi-ld:Vehicle:{number}").parse().expect("a valid identifier")
    }

    fn type_of(name: &str) -> TypeExpr {
        TypeExpr::Type(iri(name))
    }

    fn selector(ids: &[u32], pattern: Option<&str>, type_expr: Option<TypeExpr>) -> EntitySelector {
        EntitySelector::builder()
            .ids(ids.iter().copied().map(id).collect())
            .id_pattern(pattern.map(|source| Pattern::parse(source).expect("a valid pattern")))
            .type_expr(type_expr)
            .build()
    }

    fn selecting(selectors: Vec<EntitySelector>) -> EntitySelection {
        EntitySelection::builder().selectors(selectors).build()
    }

    fn filtered(q: &str) -> EntitySelection {
        EntitySelection::builder()
            .q(Some(CompiledFilter::new(parse(q).expect("a clause 4.9 filter"), LanguageMatching::TagEquality)))
            .build()
    }

    fn record(selection: &EntitySelection, scope: IdentifierScope) -> Vec<u8> {
        QueryIdentity::of(selection, None, scope).record().expect("a writable identity")
    }

    fn kept(selection: &EntitySelection) -> Vec<u8> {
        record(selection, IdentifierScope::Kept)
    }

    fn excepted(selection: &EntitySelection) -> Vec<u8> {
        record(selection, IdentifierScope::Excepted)
    }

    fn within(coordinates: &str) -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_str::<Geometry>(&format!(r#"{{"type":"Polygon","coordinates":{coordinates}}}"#)).expect("a polygon"),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("an attribute name"),
        }
    }

    /// The terms of a conjunction are a set.
    #[test]
    fn q_terms_written_in_another_order_have_one_identity() {
        assert_eq!(kept(&filtered("a==1;b==2")), kept(&filtered("b==2;a==1")));
        assert_eq!(excepted(&filtered("a==1;b==2")), excepted(&filtered("b==2;a==1")));
    }

    /// A term with another value changes the identity.
    #[test]
    fn q_terms_with_another_value_have_another_identity() {
        assert_ne!(kept(&filtered("a==1")), kept(&filtered("a==2")));
    }

    /// The normal form joins the equalities of one disjunction over one path.
    #[test]
    fn a_disjunction_of_equalities_and_a_value_list_have_one_identity() {
        assert_eq!(kept(&filtered("a==1|a==2")), kept(&filtered("a==1,2")));
    }

    /// Clause 4.6.3: a `DateTime`, a `Date` and a `Time` operand each carry
    /// their own spelling into the identity.
    #[test]
    fn q_terms_over_other_instants_dates_and_times_have_another_identity() {
        for (first, second) in [
            ("a>2024-01-15T00:00:00Z", "a>2024-01-16T00:00:00Z"),
            ("a>2024-01-15", "a>2024-01-16"),
            ("a>12:30:00Z", "a>12:31:00Z"),
        ] {
            assert_eq!(kept(&filtered(first)), kept(&filtered(first)), "{first}");
            assert_ne!(kept(&filtered(first)), kept(&filtered(second)), "{first} against {second}");
        }
    }

    #[test]
    fn identifier_lists_differ_when_kept_and_agree_when_excepted() {
        let first = selecting(vec![selector(&[1, 2], None, Some(type_of("Vehicle")))]);
        let second = selecting(vec![selector(&[3], None, Some(type_of("Vehicle")))]);
        assert_ne!(kept(&first), kept(&second));
        assert_eq!(excepted(&first), excepted(&second));
    }

    #[test]
    fn identifiers_written_in_another_order_have_one_identity() {
        let first = selecting(vec![selector(&[1, 2, 2], None, None)]);
        let second = selecting(vec![selector(&[2, 1], None, None)]);
        assert_eq!(kept(&first), kept(&second));
    }

    #[test]
    fn identifier_patterns_differ_when_kept_and_agree_when_excepted() {
        let first = selecting(vec![selector(&[], Some("^urn:ngsi-ld:Vehicle:1"), Some(type_of("Vehicle")))]);
        let second = selecting(vec![selector(&[], Some("^urn:ngsi-ld:Vehicle:2"), Some(type_of("Vehicle")))]);
        assert_ne!(kept(&first), kept(&second));
        assert_eq!(excepted(&first), excepted(&second));
    }

    /// Clause 5.2.33: `id` takes precedence over `idPattern`, so a pattern
    /// written beside identifiers selects nothing.
    #[test]
    fn an_id_pattern_beside_identifiers_is_not_part_of_the_identity() {
        let with_pattern = selecting(vec![selector(&[1], Some("^urn:nomatch"), None)]);
        let without = selecting(vec![selector(&[1], None, None)]);
        assert_eq!(kept(&with_pattern), kept(&without));
    }

    #[test]
    fn selectors_written_in_another_order_have_one_identity_when_kept() {
        let first = selecting(vec![selector(&[1], None, Some(type_of("A"))), selector(&[2], None, Some(type_of("B")))]);
        let second = selecting(vec![selector(&[2], None, Some(type_of("B"))), selector(&[1], None, Some(type_of("A")))]);
        assert_eq!(kept(&first), kept(&second));
    }

    #[test]
    fn a_selector_written_twice_is_one_selector() {
        let once = selecting(vec![selector(&[1], None, Some(type_of("A")))]);
        let twice = selecting(vec![selector(&[1], None, Some(type_of("A"))), selector(&[1], None, Some(type_of("A")))]);
        assert_eq!(kept(&once), kept(&twice));
    }

    #[test]
    fn selectors_with_other_types_have_another_identity_when_kept() {
        let first = selecting(vec![selector(&[1], None, Some(type_of("A")))]);
        let second = selecting(vec![selector(&[1], None, Some(type_of("B")))]);
        assert_ne!(kept(&first), kept(&second));
        assert_ne!(excepted(&first), excepted(&second));
    }

    /// Clause 4.17: a disjunction of types selects alike in either order.
    #[test]
    fn type_alternatives_written_in_another_order_have_one_identity_when_kept() {
        let first = selecting(vec![selector(&[1], None, Some(TypeExpr::Or(vec![type_of("A"), type_of("B")])))]);
        let second = selecting(vec![selector(&[1], None, Some(TypeExpr::Or(vec![type_of("B"), type_of("A")])))]);
        assert_eq!(kept(&first), kept(&second));
    }

    /// Clause 5.2.33: the `entities[]` selectors are alternatives of one
    /// another, as the operands of a top-level `|` are.
    #[test]
    fn two_selectors_and_one_selector_of_a_disjunction_have_one_identity_when_excepted() {
        let two = selecting(vec![selector(&[1], None, Some(type_of("A"))), selector(&[], Some("^urn:"), Some(type_of("B")))]);
        let one = selecting(vec![selector(&[], None, Some(TypeExpr::Or(vec![type_of("B"), type_of("A")])))]);
        assert_eq!(excepted(&two), excepted(&one));
        assert_ne!(kept(&two), kept(&one));
    }

    #[test]
    fn the_scope_of_the_identifiers_is_part_of_the_identity() {
        let selection = selecting(vec![selector(&[], None, Some(type_of("A")))]);
        assert_ne!(kept(&selection), excepted(&selection));
    }

    #[test]
    fn attribute_lists_written_in_another_order_have_one_identity() {
        let first = EntitySelection::builder().attrs(vec![iri("speed"), iri("brand")]).build();
        let second = EntitySelection::builder().attrs(vec![iri("brand"), iri("speed"), iri("brand")]).build();
        assert_eq!(kept(&first), kept(&second));
        assert_ne!(kept(&first), kept(&EntitySelection::builder().attrs(vec![iri("speed")]).build()));
    }

    /// An Entity a `pick` would empty is not selected, so the Attributes it
    /// keeps decide the Entities a query holds, in either scope.
    #[test]
    fn the_attributes_a_pick_keeps_are_part_of_the_identity() {
        let picking = |attributes: Vec<IriBuf>| {
            EntitySelection::builder()
                .projected_members(ProjectedMembers::AnyOf {
                    attributes,
                    scope: ScopeRetention::Removed,
                })
                .build()
        };
        let speed_and_brand = picking(vec![iri("speed"), iri("brand")]);
        assert_eq!(kept(&speed_and_brand), kept(&picking(vec![iri("brand"), iri("speed")])));
        assert_ne!(kept(&speed_and_brand), kept(&picking(vec![iri("brand")])));
        assert_ne!(excepted(&speed_and_brand), excepted(&picking(vec![iri("brand")])));
        assert_ne!(kept(&speed_and_brand), kept(&EntitySelection::builder().build()));
    }

    #[test]
    fn a_changed_geo_query_changes_the_identity() {
        let ring = "[[[0,0],[1,0],[1,1],[0,1],[0,0]]]";
        let larger = "[[[0,0],[2,0],[2,2],[0,2],[0,0]]]";
        let first = EntitySelection::builder().geo(Some(within(ring))).build();
        let second = EntitySelection::builder().geo(Some(within(larger))).build();
        assert_eq!(kept(&first), kept(&EntitySelection::builder().geo(Some(within(ring))).build()));
        assert_ne!(kept(&first), kept(&second));
        assert_ne!(kept(&first), kept(&EntitySelection::default()));
    }

    #[test]
    fn a_changed_scope_query_changes_the_identity() {
        let scoped = |expression: &str| {
            EntitySelection::builder()
                .scope_q(Some(ScopeQuery::parse(expression).expect("a scope query")))
                .build()
        };
        assert_eq!(kept(&scoped("/Madrid")), kept(&scoped("/Madrid")));
        assert_ne!(kept(&scoped("/Madrid")), kept(&scoped("/Paris")));
        assert_ne!(kept(&scoped("/Madrid")), kept(&EntitySelection::default()));
    }

    #[test]
    fn a_changed_context_source_filter_changes_the_identity() {
        let csf = |text: &str| FilterReading::of_context_source_filter(&parse(text).expect("a clause 4.9 filter"));
        let selection = EntitySelection::default();
        let identity = |reading: Option<&FilterReading>| {
            QueryIdentity::of(&selection, reading, IdentifierScope::Excepted)
                .record()
                .expect("a writable identity")
        };
        let (first, same, other) = (csf("group==\"a\""), csf("group==\"a\""), csf("group==\"b\""));
        assert_eq!(identity(Some(&first)), identity(Some(&same)));
        assert_ne!(identity(Some(&first)), identity(Some(&other)));
        assert_ne!(identity(Some(&first)), identity(None));
    }

    /// The same compact name expanded under two contexts is two types.
    #[test]
    fn a_type_that_expands_to_another_iri_changes_the_identity() {
        let first = selecting(vec![selector(&[], None, Some(type_of("Vehicle")))]);
        let second = selecting(vec![selector(
            &[],
            None,
            Some(TypeExpr::Type("https://other.example/Vehicle".parse().expect("a valid IRI"))),
        )]);
        assert_ne!(excepted(&first), excepted(&second));
    }

    /// Every issued cursor digest and every stored Entity Map fingerprint
    /// hashes this record, so a change to the shape of any type it writes
    /// invalidates both; the snapshots make such a change one reviewed diff.
    #[test]
    fn the_record_of_a_query_is_fixed() {
        let selection = EntitySelection::builder()
            .selectors(vec![
                selector(&[2, 1], None, Some(TypeExpr::Or(vec![type_of("Car"), type_of("Bus")]))),
                selector(&[], Some("^urn:ngsi-ld:Vehicle:.*"), Some(type_of("Truck"))),
            ])
            .attrs(vec![iri("speed"), iri("brand")])
            .q(Some(CompiledFilter::new(
                parse("speed>50;brand==\"x\",\"y\"|seen==2024-01-01T00:00:00Z").expect("a clause 4.9 filter"),
                LanguageMatching::TagEquality,
            )))
            .geo(Some(within("[[[0,0],[1,0],[1,1],[0,1],[0,0]]]")))
            .scope_q(Some(ScopeQuery::parse("/Madrid/#").expect("a scope query")))
            .projected_members(ProjectedMembers::AnyOf {
                attributes: vec![iri("speed")],
                scope: ScopeRetention::Removed,
            })
            .build();
        let csf = FilterReading::of_context_source_filter(&parse("tier==\"gold\"").expect("a clause 4.9 filter"));
        let written = |scope: IdentifierScope| {
            String::from_utf8(QueryIdentity::of(&selection, Some(&csf), scope).record().expect("a writable identity")).expect("a JSON record")
        };
        assert_named_snapshot!("query-identity-record-kept", written(IdentifierScope::Kept));
        assert_named_snapshot!("query-identity-record-excepted", written(IdentifierScope::Excepted));
        let retrieval = QueryIdentity::of_retrieval(Some(&type_of("Car")), &[iri("speed")])
            .record()
            .expect("a writable identity");
        assert_named_snapshot!("query-identity-record-retrieval", String::from_utf8(retrieval).expect("a JSON record"));
    }

    #[test]
    fn a_retrieval_is_identified_by_its_type_alternatives_and_attributes() {
        let identity =
            |entity_type: Option<&TypeExpr>, attrs: &[IriBuf]| QueryIdentity::of_retrieval(entity_type, attrs).record().expect("a writable identity");
        let union = TypeExpr::Or(vec![type_of("B"), type_of("A")]);
        let reversed = TypeExpr::Or(vec![type_of("A"), type_of("B")]);
        assert_eq!(identity(Some(&union), &[iri("y"), iri("x")]), identity(Some(&reversed), &[iri("x"), iri("y")]));
        assert_ne!(identity(Some(&union), &[]), identity(None, &[]));
        assert_ne!(identity(None, &[iri("x")]), identity(None, &[iri("y")]));
    }
}
