//! Clause 4.17 type selection read from a type-membership relation.
//!
//! A membership relation holds one row per type an Entity carries, keyed by
//! type and then identifier, so the Entities of one type are one key range in
//! identifier order: `entity_type_members` for current state,
//! `temporal_entity_type_members` for Temporal Evolutions. A type expression
//! therefore renders to a stream of `(key, id)` a statement can page in the
//! clause 5.5.9 `id` order by walking the key, reading only the Entities it
//! skips and keeps: one range for a type, the merge of the ranges without
//! repeats for `,`, and the first range narrowed by key probes for `;`.
//! [`crate::type_predicate`] renders the same expression as a filter on a type
//! key array instead; this module is for the statement the selection drives.
//!
//! The wildcard selects every Entity whatever its types, so it has no range to
//! walk, and neither has any expression it would have to be merged into.

use crate::{dictionary::resolved_names::ResolvedNames, entity_type_name_key::EntityTypeNameKey};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
use andromeda_postgresql_schema::type_membership::TypeMembershipRelation;
use andromeda_postgresql_seaquery::{
    branch::qualified,
    literal::{false_literal, int_literal},
};
use sea_query::{Condition, DynIden, Expr, ExprTrait, Iden, Order, Query, SelectStatement, SimpleExpr, TableRef, UnionType};

/// The merge of several ranges, `typed`.
#[derive(Iden, Clone, Copy)]
#[iden = "typed"]
struct Typed;

/// The range a conjunction narrows, `candidate`.
#[derive(Iden, Clone, Copy)]
#[iden = "candidate"]
struct Candidate;

/// The membership row a key probe reads, `type_member`.
#[derive(Iden, Clone, Copy)]
#[iden = "type_member"]
struct TypeMember;

/// The membership relation a walk or a probe reads, in its branch.
#[derive(Clone, Copy, Debug)]
pub struct Memberships<'a> {
    /// Which relation.
    pub relation: TypeMembershipRelation,
    /// The branch schema it is named in, `None` for the live branch.
    pub schema: Option<&'a DynIden>,
}

impl Memberships<'_> {
    /// The relation, qualified by its branch schema.
    pub(crate) fn table(self) -> TableRef {
        qualified(self.schema, self.relation.table())
    }
}

/// Whether `expr` has a range to walk: every leaf it would merge names a type,
/// and every conjunction has one such child to narrow.
#[must_use]
pub fn drives(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Wildcard => false,
        TypeExpr::Type(_) => true,
        TypeExpr::Or(children) => children.iter().all(drives),
        TypeExpr::And(children) => children.iter().any(drives),
    }
}

/// The Entities any of `exprs` selects, as `(key, id)` with no repeat, or
/// `None` when one of them has no range to walk (see [`drives`]).
///
/// Several expressions are several selectors of one request (clause 5.2.33),
/// which select their union exactly as a `,` does.
#[must_use]
pub fn members_of_any<'a>(
    exprs: impl IntoIterator<Item = &'a TypeExpr>,
    types: &ResolvedNames<EntityTypeName>,
    memberships: Memberships<'_>,
) -> Option<SelectStatement> {
    let streams = exprs.into_iter().map(|expr| members_of(expr, types, memberships)).collect::<Option<Vec<_>>>()?;
    Some(merged(streams, memberships))
}

/// The Entities `expr` selects, as `(key, id)` with no repeat, or `None` when
/// it has no range to walk.
fn members_of(expr: &TypeExpr, types: &ResolvedNames<EntityTypeName>, memberships: Memberships<'_>) -> Option<SelectStatement> {
    let relation = memberships.relation;
    match expr {
        TypeExpr::Wildcard => None,
        TypeExpr::Type(iri) => Some(range(types.get_str(iri.as_str()).map(EntityTypeNameKey::get), memberships)),
        TypeExpr::Or(children) => members_of_any(children, types, memberships),
        TypeExpr::And(children) => {
            let (driving, stream) = children
                .iter()
                .enumerate()
                .find_map(|(position, child)| members_of(child, types, memberships).map(|stream| (position, stream)))?;
            let id = Expr::col((Candidate, relation.id()));
            let narrowing = children
                .iter()
                .enumerate()
                .filter(|(position, _)| *position != driving)
                .fold(Condition::all(), |narrowing, (_, child)| {
                    narrowing.add(membership(child, types, &id, memberships))
                });
            Some(
                Query::select()
                    .column((Candidate, relation.key()))
                    .column((Candidate, relation.id()))
                    .from_subquery(stream, Candidate)
                    .cond_where(narrowing)
                    .take(),
            )
        }
    }
}

/// Whether the Entity identified by `id` carries types `expr` selects, as key
/// probes of the membership relation: the filter form of [`members_of_any`],
/// for a statement that walks some other range.
#[must_use]
pub fn membership(expr: &TypeExpr, types: &ResolvedNames<EntityTypeName>, id: &SimpleExpr, memberships: Memberships<'_>) -> Condition {
    let relation = memberships.relation;
    match expr {
        TypeExpr::Wildcard => Condition::all(),
        TypeExpr::Type(iri) => Condition::all().add(match types.get_str(iri.as_str()) {
            Some(key) => Expr::exists(
                Query::select()
                    .expr(int_literal(1))
                    .from_as(memberships.table(), TypeMember)
                    .and_where(Expr::col((TypeMember, relation.type_key())).eq(int_literal(i64::from(key.get()))))
                    .and_where(Expr::col((TypeMember, relation.id())).eq(id.clone())) // clone: every leaf probes the same Entity
                    .take(),
            ),
            None => false_literal(),
        }),
        TypeExpr::And(children) => children
            .iter()
            .fold(Condition::all(), |all, child| all.add(membership(child, types, id, memberships))),
        TypeExpr::Or(children) => children
            .iter()
            .fold(Condition::any(), |any, child| any.add(membership(child, types, id, memberships))),
    }
}

/// The key range of one type, or no row for a type the dictionary never
/// recorded: no Entity can carry a type that was never interned.
///
/// The key is inlined, as the type-array predicate inlines it, so the planner
/// estimates the range from the statistics of that one type.
fn range(key: Option<i32>, memberships: Memberships<'_>) -> SelectStatement {
    let relation = memberships.relation;
    let mut range = Query::select();
    range.column(relation.key()).column(relation.id()).from(memberships.table());
    match key {
        Some(key) => range.and_where(Expr::col(relation.type_key()).eq(int_literal(i64::from(key)))),
        None => range.and_where(false_literal()),
    };
    range.take()
}

/// The merge of several streams in identifier order, each Entity once: an
/// Entity carrying two of the types is in two ranges (clause 4.17 selects on
/// membership, not on a single type).
///
/// `DISTINCT ON` over the ordered union is what lets PostgreSQL merge the
/// ranges as they stream rather than collecting and sorting them.
fn merged(streams: Vec<SelectStatement>, memberships: Memberships<'_>) -> SelectStatement {
    let relation = memberships.relation;
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
        .distinct_on([(Typed, relation.id())])
        .column((Typed, relation.key()))
        .column((Typed, relation.id()))
        .from_subquery(union, Typed)
        .order_by((Typed, relation.id()), Order::Asc)
        .take()
}

#[cfg(test)]
mod tests {
    use crate::{
        dictionary::resolved_names::ResolvedNames,
        entity_type_name_key::EntityTypeNameKey,
        type_membership::{Memberships, drives, members_of_any, membership},
    };
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
    use andromeda_postgresql_schema::type_membership::TypeMembershipRelation;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Condition, Expr, IntoIden, PostgresQueryBuilder, Query};

    const CURRENT: Memberships<'static> = Memberships {
        relation: TypeMembershipRelation::Current,
        schema: None,
    };

    fn ty(name: &str) -> TypeExpr {
        TypeExpr::Type(IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI"))
    }

    fn known() -> ResolvedNames<EntityTypeName> {
        ResolvedNames::from_pairs([("A", 1), ("B", 2), ("C", 3)].into_iter().map(|(name, key)| {
            (
                format!("https://example.com/{name}").parse::<EntityTypeName>().expect("valid type name"),
                EntityTypeNameKey::from(key),
            )
        }))
    }

    fn stream(exprs: &[TypeExpr]) -> String {
        members_of_any(exprs, &known(), CURRENT)
            .expect("the selection has a range")
            .to_string(PostgresQueryBuilder)
    }

    /// Clause 4.17: one type is one key range, walked in identifier order.
    #[test]
    fn one_type_is_one_key_range() {
        assert_eq!(
            stream(&[ty("A")]),
            r#"SELECT "entity_key", "id" FROM "entity_type_members" WHERE "entity_type_name_key" = (1)"#
        );
    }

    /// A type never interned is carried by no Entity, so its range is empty.
    #[test]
    fn an_unknown_type_has_an_empty_range() {
        assert_eq!(stream(&[ty("Unknown")]), r#"SELECT "entity_key", "id" FROM "entity_type_members" WHERE FALSE"#);
    }

    /// Clause 4.17 `,`: the ranges merge in identifier order, and an Entity
    /// carrying both types is selected once.
    #[test]
    fn a_disjunction_merges_its_ranges_without_repeats() {
        let sql = stream(&[TypeExpr::Or(vec![ty("A"), ty("B")])]);
        assert!(
            sql.starts_with(r#"SELECT DISTINCT ON ("typed"."id") "typed"."entity_key", "typed"."id" FROM ("#),
            "{sql}"
        );
        assert!(sql.contains(r#""entity_type_name_key" = (1) UNION ALL (SELECT"#), "{sql}");
        assert!(sql.contains(r#""entity_type_name_key" = (2))"#), "{sql}");
        assert!(sql.ends_with(r#") AS "typed" ORDER BY "typed"."id" ASC"#), "{sql}");
    }

    /// Several selectors select their union, as a disjunction does.
    #[test]
    fn several_selectors_merge_as_a_disjunction_does() {
        assert_eq!(stream(&[ty("A"), ty("B")]), stream(&[TypeExpr::Or(vec![ty("A"), ty("B")])]));
    }

    /// Clause 4.17 `;`: the first range is narrowed by a key probe per other
    /// type, so an Entity must carry every one.
    #[test]
    fn a_conjunction_narrows_its_first_range_by_key_probes() {
        let sql = stream(&[TypeExpr::And(vec![ty("A"), ty("B")])]);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "candidate"."entity_key", "candidate"."id" FROM "#,
                r#"(SELECT "entity_key", "id" FROM "entity_type_members" WHERE "entity_type_name_key" = (1)) AS "candidate" "#,
                r#"WHERE EXISTS(SELECT 1 FROM "entity_type_members" AS "type_member" "#,
                r#"WHERE "type_member"."entity_type_name_key" = (2) AND "type_member"."id" = "candidate"."id")"#
            )
        );
    }

    /// The wildcard selects every Entity, so neither it nor a disjunction
    /// containing it has a range; a conjunction still walks its typed child.
    #[test]
    fn the_wildcard_has_no_range_unless_a_conjunction_names_a_type() {
        assert!(!drives(&TypeExpr::Wildcard));
        assert!(!drives(&TypeExpr::Or(vec![ty("A"), TypeExpr::Wildcard])));
        assert!(drives(&TypeExpr::And(vec![TypeExpr::Wildcard, ty("A")])));
        assert!(members_of_any([&TypeExpr::Or(vec![ty("A"), TypeExpr::Wildcard])], &known(), CURRENT).is_none());
        assert_eq!(
            stream(&[TypeExpr::And(vec![TypeExpr::Wildcard, ty("A")])]),
            concat!(
                r#"SELECT "candidate"."entity_key", "candidate"."id" FROM "#,
                r#"(SELECT "entity_key", "id" FROM "entity_type_members" WHERE "entity_type_name_key" = (1)) AS "candidate" WHERE TRUE"#
            )
        );
    }

    /// The filter form probes the membership key of the Entity it is given,
    /// keeping the expression's boolean structure.
    #[test]
    fn the_filter_form_probes_the_membership_key_per_leaf() {
        let id = Expr::col((Alias::new("selected"), Alias::new("id")));
        let expr = TypeExpr::Or(vec![TypeExpr::And(vec![ty("A"), ty("B")]), ty("Unknown")]);
        let sql = Query::select()
            .expr(Expr::val(1))
            .cond_where(Condition::all().add(membership(&expr, &known(), &id, CURRENT)))
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql.matches(r#""type_member"."id" = "selected"."id""#).count(), 2, "{sql}");
        assert!(sql.contains(") OR (FALSE)"), "{sql}");
    }

    /// A Snapshot branch walks its own schema's memberships.
    #[test]
    fn a_branch_walks_its_own_memberships() {
        let schema = Alias::new("snap_5").into_iden();
        let memberships = Memberships {
            relation: TypeMembershipRelation::Current,
            schema: Some(&schema),
        };
        let sql = members_of_any([&TypeExpr::And(vec![ty("A"), ty("B")])], &known(), memberships)
            .expect("the selection has a range")
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql.matches(r#""snap_5"."entity_type_members""#).count(), 2, "{sql}");
    }

    /// A Temporal Evolution's types are walked in their own relation, keyed by
    /// the temporal catalogue's surrogate key.
    #[test]
    fn a_temporal_selection_walks_the_temporal_memberships() {
        let temporal = Memberships {
            relation: TypeMembershipRelation::Temporal,
            schema: None,
        };
        let sql = members_of_any([&TypeExpr::And(vec![ty("A"), ty("B")])], &known(), temporal)
            .expect("the selection has a range")
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "candidate"."temporal_entity_key", "candidate"."id" FROM "#,
                r#"(SELECT "temporal_entity_key", "id" FROM "temporal_entity_type_members" WHERE "entity_type_name_key" = (1)) AS "candidate" "#,
                r#"WHERE EXISTS(SELECT 1 FROM "temporal_entity_type_members" AS "type_member" "#,
                r#"WHERE "type_member"."entity_type_name_key" = (2) AND "type_member"."id" = "candidate"."id")"#
            )
        );
    }
}
