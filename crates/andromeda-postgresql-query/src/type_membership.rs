//! Clause 4.17 type selection read from the type-membership relation.
//!
//! `entity_type_members` holds one row per type an Entity carries, keyed by
//! type and then identifier, so the Entities of one type are one key range in
//! identifier order; a walk keeps the rows of the side it reads, the current
//! state for a core read and the Temporal Evolution for a temporal one, so
//! the index carries both flags and the walk never visits the heap. A type selection
//! therefore renders to a stream of `(key, id)` a statement can page in the
//! clause 5.5.9 `id` order by walking the key, reading only the Entities it
//! skips and keeps: one range for a type, and the first range narrowed by key
//! probes for `;`. A `,` selects the union of its alternatives' ranges, each
//! Entity once, either as their merge without repeats or as disjoint streams,
//! each an alternative's range less the Entities an earlier alternative
//! selects. PostgreSQL merges no two ranges of one index in order, so the
//! merge sorts every member before its first row, where each stream reads in
//! the order of its range and a statement can stop each at the rows it needs.
//! [`crate::type_predicate`] renders the same expression as a filter on a type
//! key array instead; this module is for the statement the selection drives.
//!
//! The wildcard selects every Entity whatever its types, so it has no range to
//! walk, and neither has any expression it would have to be merged into.

use crate::type_predicate::selection_condition;
use andromeda_ngsi_ld_lexicon::{entity_type_iri::EntityTypeIri, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_query::{name_stage::Expanded, type_selection::TypeSelection};
use andromeda_postgresql_schema::current::EntityTypeMembers;
use andromeda_postgresql_seaquery::literal::{false_literal, int_literal};
use andromeda_postgresql_stored_form::memberships::Memberships;
use andromeda_postgresql_stored_values::{entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
use sea_query::{Condition, Expr, ExprTrait, Iden, Order, Query, SelectStatement, SimpleExpr, UnionType};

/// The merge of several ranges, `typed`.
#[derive(Iden, Clone, Copy)]
#[iden = "typed"]
struct Typed;

/// The range a conjunction narrows, `candidate`.
#[derive(Iden, Clone, Copy)]
#[iden = "candidate"]
struct Candidate;

/// One alternative's members no alternative before it selects, `unseen`.
#[derive(Iden, Clone, Copy)]
#[iden = "unseen"]
struct Unseen;

/// The membership row a key probe reads, `type_member`.
#[derive(Iden, Clone, Copy)]
#[iden = "type_member"]
struct TypeMember;

/// Whether `expr` has a range to walk: every alternative names a type, which
/// every selection but the wildcard does.
#[must_use]
pub const fn drives(expr: &TypeSelection<Expanded>) -> bool {
    match expr {
        TypeSelection::Wildcard => false,
        TypeSelection::Alternatives(_) => true,
    }
}

/// The Entities any of `exprs` selects, as `(key, id)` with no repeat, or
/// `None` when one of them has no range to walk (see [`drives`]).
///
/// Several expressions are several selectors of one request (clause 5.2.33),
/// which select their union exactly as a `,` does.
#[must_use]
pub fn members_of_any<'a>(
    exprs: impl IntoIterator<Item = &'a TypeSelection<Expanded>>,
    types: &ResolvedNames<EntityTypeName>,
    memberships: Memberships<'_>,
) -> Option<SelectStatement> {
    let streams = exprs.into_iter().map(|expr| members_of(expr, types, memberships)).collect::<Option<Vec<_>>>()?;
    Some(merged(streams, memberships))
}

/// The Entities any of `exprs` selects, as one stream of `(key, id)` per
/// alternative, each a range in identifier order holding the alternative's
/// Entities that no alternative before it selects: the first stream and the
/// rest, none for a selection of one alternative. `None` when `exprs` holds
/// no alternative or one of them has no range to walk (see [`drives`]).
///
/// The streams are disjoint and their union is what [`members_of_any`]
/// merges, so a count of every Entity the selection selects under a filter
/// of each Entity is the sum of the counts of the streams, and the first `n`
/// of the selection in an order are among the first `n` of each stream in that
/// order. Each stream is a range of the membership index, its members tested
/// by key against the types of the alternatives before it. Merged in
/// identifier order instead, the ranges are sorted whole: the 757 134 members
/// of two types of a GTFS feed spill a 47 MB sort, counted in 510 ms where
/// their two streams take 46 ms, and paged in 260 ms for the first row.
#[must_use]
pub fn disjoint_members_of_any<'a>(
    exprs: impl IntoIterator<Item = &'a TypeSelection<Expanded>>,
    types: &ResolvedNames<EntityTypeName>,
    memberships: Memberships<'_>,
) -> Option<(SelectStatement, Vec<SelectStatement>)> {
    let alternatives = exprs
        .into_iter()
        .map(|expr| match expr {
            TypeSelection::Wildcard => None,
            TypeSelection::Alternatives(alternatives) => Some(alternatives.iter()),
        })
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<&[EntityTypeIri]>>();
    let mut streams = alternatives
        .iter()
        .enumerate()
        .map(|(position, conjunction)| {
            let stream = members_of_conjunction(conjunction, types, memberships)?;
            let earlier = alternatives.get(..position).unwrap_or_default();
            if earlier.is_empty() {
                return Some(stream);
            }
            let id = Expr::col((Unseen, EntityTypeMembers::Id));
            // Each earlier alternative is excluded on its own, so one naming a
            // single type is a `NOT EXISTS` PostgreSQL can answer as an
            // anti-join of the two ranges.
            let unselected_before = earlier.iter().fold(Condition::all(), |unselected, conjunction| {
                unselected.add(
                    conjunction
                        .iter()
                        .fold(Condition::all(), |carried, iri| carried.add(type_probe(iri, types, &id, memberships)))
                        .not(),
                )
            });
            Some(
                Query::select()
                    .column((Unseen, EntityTypeMembers::EntityKey))
                    .column((Unseen, EntityTypeMembers::Id))
                    .from_subquery(stream, Unseen)
                    .cond_where(unselected_before)
                    .take(),
            )
        })
        .collect::<Option<Vec<SelectStatement>>>()?
        .into_iter();
    let first = streams.next()?;
    Some((first, streams.collect()))
}

/// The Entities `expr` selects, as `(key, id)` with no repeat, or `None` when
/// it has no range to walk.
fn members_of(expr: &TypeSelection<Expanded>, types: &ResolvedNames<EntityTypeName>, memberships: Memberships<'_>) -> Option<SelectStatement> {
    let TypeSelection::Alternatives(alternatives) = expr else {
        return None;
    };
    let streams = alternatives
        .iter()
        .map(|conjunction| members_of_conjunction(conjunction, types, memberships))
        .collect::<Option<Vec<_>>>()?;
    Some(merged(streams, memberships))
}

/// The Entities carrying every one of `conjunction`, as `(key, id)`: the range
/// of its first type, narrowed by a key probe per other type. `None` when it
/// names no type.
fn members_of_conjunction(conjunction: &[EntityTypeIri], types: &ResolvedNames<EntityTypeName>, memberships: Memberships<'_>) -> Option<SelectStatement> {
    let (driving, others) = conjunction.split_first()?;
    let stream = range(types.get_str(driving.as_str()).map(EntityTypeNameKey::get), memberships);
    if others.is_empty() {
        return Some(stream);
    }
    let id = Expr::col((Candidate, EntityTypeMembers::Id));
    let narrowing = others
        .iter()
        .fold(Condition::all(), |narrowing, other| narrowing.add(type_probe(other, types, &id, memberships)));
    Some(
        Query::select()
            .column((Candidate, EntityTypeMembers::EntityKey))
            .column((Candidate, EntityTypeMembers::Id))
            .from_subquery(stream, Candidate)
            .cond_where(narrowing)
            .take(),
    )
}

/// Whether the Entity identified by `id` carries types `expr` selects, as key
/// probes of the membership relation: the filter form of [`members_of_any`],
/// for a statement that walks some other range.
#[must_use]
pub fn membership(expr: &TypeSelection<Expanded>, types: &ResolvedNames<EntityTypeName>, id: &SimpleExpr, memberships: Memberships<'_>) -> Condition {
    selection_condition(expr, |iri| type_probe(iri, types, id, memberships))
}

/// Whether the Entity identified by `id` carries the type `iri`, as one key
/// probe of the membership relation, or constant `FALSE` for a type the
/// dictionary never recorded.
fn type_probe(iri: &EntityTypeIri, types: &ResolvedNames<EntityTypeName>, id: &SimpleExpr, memberships: Memberships<'_>) -> Condition {
    Condition::all().add(match types.get_str(iri.as_str()) {
        Some(key) => Expr::exists(
            Query::select()
                .expr(int_literal(1))
                .from_as(memberships.table(), TypeMember)
                .and_where(Expr::col((TypeMember, EntityTypeMembers::EntityTypeNameKey)).eq(int_literal(i64::from(key.get()))))
                .and_where(Expr::col((TypeMember, EntityTypeMembers::Id)).eq(id.clone())) // clone: every leaf probes the same Entity
                .and_where(Expr::col((TypeMember, memberships.side.flag())))
                .take(),
        ),
        None => false_literal(),
    })
}

/// The key range of one type on the side `memberships` reads, or no row for
/// a type the dictionary never recorded: no Entity can carry a type that was
/// never interned.
///
/// The key is inlined, as the type-array predicate inlines it, so the planner
/// estimates the range from the statistics of that one type.
fn range(key: Option<i32>, memberships: Memberships<'_>) -> SelectStatement {
    let mut range = Query::select();
    range
        .column(EntityTypeMembers::EntityKey)
        .column(EntityTypeMembers::Id)
        .from(memberships.table());
    match key {
        Some(key) => range
            .and_where(Expr::col(EntityTypeMembers::EntityTypeNameKey).eq(int_literal(i64::from(key))))
            .and_where(Expr::col(memberships.side.flag())),
        None => range.and_where(false_literal()),
    };
    range.take()
}

/// The merge of several streams in identifier order, each Entity once: an
/// Entity carrying two of the types is in two ranges (clause 4.17 selects on
/// membership, not on a single type).
///
/// `DISTINCT ON` over the union keeps the first row of each identifier, which
/// PostgreSQL reads by sorting the whole union: it merges no two ranges of one
/// index in order, so the merge yields its first row once every member is
/// sorted. A statement that stops at its first rows reads
/// [`disjoint_members_of_any`] instead.
fn merged(streams: Vec<SelectStatement>, memberships: Memberships<'_>) -> SelectStatement {
    let mut streams = streams.into_iter();
    let Some(first) = streams.next() else {
        return range(None, memberships);
    };
    let mut rest = streams.peekable();
    if rest.peek().is_none() {
        return first;
    }
    let union = rest.fold(first, |mut union, stream| {
        union.union(UnionType::All, stream);
        union
    });
    Query::select()
        .distinct_on([(Typed, EntityTypeMembers::Id)])
        .column((Typed, EntityTypeMembers::EntityKey))
        .column((Typed, EntityTypeMembers::Id))
        .from_subquery(union, Typed)
        .order_by((Typed, EntityTypeMembers::Id), Order::Asc)
        .take()
}

#[cfg(test)]
mod tests {
    use crate::type_membership::{disjoint_members_of_any, drives, members_of_any, membership};
    use andromeda_ngsi_ld_lexicon::{entity_type_iri::EntityTypeIri, entity_type_name::EntityTypeName};
    use andromeda_ngsi_ld_query::{name_stage::Expanded, type_selection::TypeSelection};
    use andromeda_postgresql_schema::type_membership::MembershipSide;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_form::memberships::Memberships;
    use andromeda_postgresql_stored_values::{entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Condition, Expr, IntoIden, Query, SelectStatement};

    const CURRENT: Memberships<'static> = Memberships {
        side: MembershipSide::Current,
        schema: None,
    };

    fn iri(name: &str) -> EntityTypeIri {
        format!("https://example.com/{name}").parse().expect("valid IRI")
    }

    fn ty(name: &str) -> TypeSelection<Expanded> {
        TypeSelection::single(iri(name))
    }

    fn alternatives(shape: &[&[&str]]) -> TypeSelection<Expanded> {
        TypeSelection::of_alternatives(shape.iter().map(|conjunction| conjunction.iter().map(|name| iri(name)).collect()).collect()).expect("a non-empty shape")
    }

    fn known() -> ResolvedNames<EntityTypeName> {
        ResolvedNames::from_pairs([("A", 1), ("B", 2), ("C", 3)].into_iter().map(|(name, key)| {
            (
                format!("https://example.com/{name}").parse::<EntityTypeName>().expect("valid type name"),
                EntityTypeNameKey::from(key),
            )
        }))
    }

    fn stream(exprs: &[TypeSelection<Expanded>]) -> SelectStatement {
        members_of_any(exprs, &known(), CURRENT).expect("the selection has a range")
    }

    /// Clause 4.17: one type is one key range, walked in identifier order.
    #[test]
    fn one_type_is_one_key_range() {
        assert_statement_snapshot!("type-membership-one-type-is-one-key-range", stream(&[ty("A")]));
    }

    /// A type never interned is carried by no Entity, so its range is empty.
    #[test]
    fn an_unknown_type_has_an_empty_range() {
        assert_statement_snapshot!("type-membership-an-unknown-type-has-an-empty-range", stream(&[ty("Unknown")]));
    }

    /// Clause 4.17 `,`: the ranges merge in identifier order, and an Entity
    /// carrying both types is selected once.
    #[test]
    fn a_disjunction_merges_its_ranges_without_repeats() {
        assert_statement_snapshot!(
            "type-membership-a-disjunction-merges-its-ranges-without-repeats",
            stream(&[alternatives(&[&["A"], &["B"]])])
        );
    }

    /// Several selectors select their union, as a disjunction does.
    #[test]
    fn several_selectors_merge_as_a_disjunction_does() {
        assert_eq!(stream(&[ty("A"), ty("B")]), stream(&[alternatives(&[&["A"], &["B"]])]));
    }

    /// Every alternative after the first keeps only the Entities no
    /// alternative before it selects, so each Entity is in one stream.
    #[test]
    fn each_alternative_keeps_the_entities_no_earlier_one_selects() {
        let (first, rest) =
            disjoint_members_of_any([&alternatives(&[&["A"], &["B", "C"]]), &ty("Unknown")], &known(), CURRENT).expect("the selection has ranges");
        assert_eq!(rest.len(), 2);
        assert_eq!(first, stream(&[ty("A")]));
        for (index, part) in [first].into_iter().chain(rest).enumerate() {
            assert_statement_snapshot!(
                format!("type-membership-each-alternative-keeps-the-entities-no-earlier-one-selects-{}", index + 1),
                part
            );
        }
    }

    /// The wildcard has no range, so a union holding it has no streams, and
    /// one alternative is one stream, its walk.
    #[test]
    fn a_union_with_the_wildcard_has_no_streams_and_one_alternative_is_its_walk() {
        assert!(disjoint_members_of_any([&ty("A"), &TypeSelection::Wildcard], &known(), CURRENT).is_none());
        let conjunction = alternatives(&[&["A", "B"]]);
        assert_eq!(
            disjoint_members_of_any([&conjunction], &known(), CURRENT),
            Some((stream(&[alternatives(&[&["A", "B"]])]), Vec::new()))
        );
    }

    /// Clause 4.17: an alternative after a conjunction leaves out the
    /// Entities carrying every type of the conjunction, probing each by key.
    #[test]
    fn a_type_beside_a_conjunction_leaves_out_the_entities_of_the_conjunction() {
        let (first, rest) = disjoint_members_of_any([&alternatives(&[&["A", "B"], &["C"]])], &known(), CURRENT).expect("the selection has ranges");
        assert_eq!(first, stream(&[alternatives(&[&["A", "B"]])]));
        let [later] = <[SelectStatement; 1]>::try_from(rest).expect("one stream after the first");
        assert_statement_snapshot!("type-membership-a-type-beside-a-conjunction-leaves-out-the-entities-of-the-conjunction", later);
    }

    /// Clause 4.17 `;`: the first range is narrowed by a key probe per other
    /// type, so an Entity must carry every one.
    #[test]
    fn a_conjunction_narrows_its_first_range_by_key_probes() {
        assert_statement_snapshot!(
            "type-membership-a-conjunction-narrows-its-first-range-by-key-probes",
            stream(&[alternatives(&[&["A", "B"]])])
        );
    }

    /// The wildcard selects every Entity, so it has no range to walk, and
    /// neither has a union that holds it.
    #[test]
    fn the_wildcard_has_no_range() {
        assert!(!drives(&TypeSelection::Wildcard));
        assert!(drives(&ty("A")));
        assert!(members_of_any([&TypeSelection::Wildcard], &known(), CURRENT).is_none());
        assert!(members_of_any([&ty("A"), &TypeSelection::Wildcard], &known(), CURRENT).is_none());
    }

    /// The filter form probes the membership key of the Entity it is given,
    /// keeping the expression's boolean structure.
    #[test]
    fn the_filter_form_probes_the_membership_key_per_leaf() {
        let id = Expr::col((Alias::new("selected"), Alias::new("id")));
        let expr = alternatives(&[&["A", "B"], &["Unknown"]]);
        let statement = Query::select()
            .expr(Expr::val(1))
            .cond_where(Condition::all().add(membership(&expr, &known(), &id, CURRENT)))
            .to_owned();
        assert_statement_snapshot!("type-membership-the-filter-form-probes-the-membership-key-per-leaf", statement);
    }

    /// A Snapshot branch walks its own schema's memberships.
    #[test]
    fn a_branch_walks_its_own_memberships() {
        let schema = Alias::new("snap_5").into_iden();
        let memberships = Memberships {
            side: MembershipSide::Current,
            schema: Some(&schema),
        };
        let statement = members_of_any([&alternatives(&[&["A", "B"]])], &known(), memberships).expect("the selection has a range");
        assert_statement_snapshot!("type-membership-a-branch-walks-its-own-memberships", statement);
    }

    /// A Temporal Evolution's types are walked in the same relation, keeping
    /// the rows of the Evolution's side.
    #[test]
    fn a_temporal_selection_walks_the_evolution_memberships() {
        let temporal = Memberships {
            side: MembershipSide::Evolution,
            schema: None,
        };
        let statement = members_of_any([&alternatives(&[&["A", "B"]])], &known(), temporal).expect("the selection has a range");
        assert_statement_snapshot!("type-membership-a-temporal-selection-walks-the-evolution-memberships", statement);
    }
}
