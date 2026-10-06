//! The entity selectors of a query (clause 5.2.33) as a filter on the rows the
//! statement is driven by.
//!
//! The selectors are unioned, `(sel1) OR (sel2) OR ...`, and each selector ANDs
//! its own `id`, `idPattern` and type. How the type is tested depends on what
//! drives the statement. Driven by `entities`, it is a containment test on the
//! search row. Driven by the type-membership walk of the selectors' own types,
//! the walk has already selected exactly the union of those types, so a
//! selector with nothing but a type adds nothing; a lone selector keeps only
//! its pattern; and several selectors that pair a type with a pattern keep
//! each pairing as key probes of the membership relation, since the walk alone
//! cannot tell which pattern goes with which type.

use crate::entity_relations::RowSource;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::{
    entity_selection::{EntitySelection, EntitySelector},
    identifier_axis::IdentifierAxis,
};
use andromeda_postgresql_query::{
    dictionary::resolved_names::ResolvedNames,
    entity_row_refs::EntityRowRefs,
    type_membership::membership,
    type_predicate::condition as type_condition,
};
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::{
    operator::regex_match,
    pg_array::{any, text_array},
};
use sea_query::{Condition, Expr, ExprTrait};

/// How a selector's type is tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeTest {
    /// A containment test on the search row's type keys.
    SearchRow,
    /// Key probes of the membership relation for the Entity's identifier.
    MembershipProbe,
    /// Nothing: the walk driving the statement selected the type already.
    Walked,
}

/// The selectors of `selection` as a filter, or `None` when they place no
/// constraint on the rows `source` yields.
///
/// A union that contains an unconstrained selector places no constraint, so it
/// is rendered only when every selector constrains, the reading
/// `EntitySelection::selectors_constrain` shares with the in-memory residual.
#[must_use]
pub fn condition(selection: &EntitySelection, rows: &EntityRowRefs, types: &ResolvedNames<EntityTypeName>, source: &RowSource) -> Option<Condition> {
    if !selection.selectors_constrain() {
        return None;
    }
    let test = match source {
        RowSource::Entities => TypeTest::SearchRow,
        RowSource::TypeMembers { .. } => match selection.selectors.as_slice() {
            selectors if selectors.iter().all(|selector| selector.id_pattern.is_none()) => return None,
            [_] => TypeTest::Walked,
            _ => TypeTest::MembershipProbe,
        },
    };
    Some(
        selection
            .selectors
            .iter()
            .fold(Condition::any(), |union, selector| union.add(selector_condition(rows, selector, types, test))),
    )
}

/// Render one selector's identifier, pattern and type predicates, `ANDed`.
///
/// The three axes of a single [`EntitySelector`] hold together (clause 5.2.33):
/// `{id:A,type:T}` selects entity `A` *and only if* it is a `T`.
fn selector_condition(rows: &EntityRowRefs, selector: &EntitySelector, types: &ResolvedNames<EntityTypeName>, test: TypeTest) -> Condition {
    let mut condition = Condition::all();
    match selector.identifier_axis() {
        IdentifierAxis::Ids(ids) => {
            let ids: Vec<&str> = ids.iter().map(AsRef::as_ref).collect();
            // A warm spatial prefilter can push tens of thousands of candidate ids
            // here; `IN ($1, … $N)` would bind one parameter per id and blow past
            // PostgreSQL's 65535 parameter ceiling. A single bound `text[]` with
            // `= ANY($1)` keeps it to one parameter regardless of set size.
            condition = condition.add(rows.identity_column(Entities::Id).eq(any(text_array(&ids))));
        }
        IdentifierAxis::Pattern(pattern) => {
            condition = condition.add(regex_match(rows.identity_column(Entities::Id), Expr::val(pattern.portable().as_str())));
        }
        IdentifierAxis::Any => {}
    }
    if let Some(type_expr) = &selector.type_expr {
        match test {
            TypeTest::SearchRow => {
                condition = condition.add(type_condition(type_expr, &rows.search_column(EntitySearch::EntityTypeNameKeys), types));
            }
            TypeTest::MembershipProbe => {
                condition = condition.add(membership(type_expr, types, &rows.identity_column(Entities::Id), rows.schema()));
            }
            TypeTest::Walked => {}
        }
    }
    condition
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_relations::{Demand, RowSource},
        selector_predicate::condition,
    };
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::{
        entity_selection::{EntitySelection, EntitySelector},
        pattern::Pattern,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_postgresql_query::{
        dictionary::resolved_names::ResolvedNames,
        entity_row_refs::{DocumentsRef, EntityRowRefs},
        entity_type_name_key::EntityTypeNameKey,
    };
    use iri_rs::IriBuf;
    use sea_query::{Alias, Condition, Expr, PostgresQueryBuilder, Query};

    fn ty(name: &str) -> TypeExpr {
        TypeExpr::Type(IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI"))
    }

    fn types() -> ResolvedNames<EntityTypeName> {
        ResolvedNames::from_pairs([("A", 1), ("B", 2)].into_iter().map(|(name, key)| {
            (
                format!("https://example.com/{name}").parse::<EntityTypeName>().expect("valid type name"),
                EntityTypeNameKey::from(key),
            )
        }))
    }

    fn walked() -> RowSource {
        RowSource::TypeMembers {
            walk: Box::new(Query::select().take()),
            entities: Demand::NotJoined,
        }
    }

    fn pattern(raw: &str) -> Pattern {
        Pattern::parse(raw).expect("valid pattern")
    }

    fn render(selectors: Vec<EntitySelector>, source: &RowSource) -> Option<String> {
        let selection = EntitySelection::builder().selectors(selectors).build();
        let rows = EntityRowRefs::unaliased_with(None, DocumentsRef::ByKeyLookup).keyed_by(Alias::new("selected"));
        condition(&selection, &rows, &types(), source)
            .map(|condition: Condition| Query::select().expr(Expr::val(1)).cond_where(condition).to_string(PostgresQueryBuilder))
    }

    /// Driven by `entities`, a type is a containment test on the search row.
    #[test]
    fn a_type_on_an_entities_driven_statement_tests_the_search_row() {
        let sql = render(vec![EntitySelector::builder().type_expr(Some(ty("A"))).build()], &RowSource::Entities).expect("constrains");
        assert!(sql.contains(r#""entity_search"."entity_type_name_keys" @> (ARRAY[1])"#), "{sql}");
    }

    /// The walk selected exactly the selectors' types, so type-only selectors
    /// add no filter at all.
    #[test]
    fn type_only_selectors_add_nothing_to_the_walk() {
        let selectors = vec![
            EntitySelector::builder().type_expr(Some(ty("A"))).build(),
            EntitySelector::builder().type_expr(Some(ty("B"))).build(),
        ];
        assert_eq!(render(selectors, &walked()), None);
    }

    /// A lone selector's pattern filters the walk's identifier; its type is the walk.
    #[test]
    fn a_lone_selector_keeps_only_its_pattern_on_the_walk() {
        let sql = render(
            vec![EntitySelector::builder().type_expr(Some(ty("A"))).id_pattern(Some(pattern("urn:a.*"))).build()],
            &walked(),
        )
        .expect("constrains");
        assert!(sql.contains(r#""selected"."id" ~ "#), "{sql}");
        assert!(!sql.contains("entity_type"), "{sql}");
    }

    /// Clause 5.2.33: several selectors pairing a type with a pattern keep each
    /// pairing, the type probed by key for the walk's identifier.
    #[test]
    fn several_selectors_keep_each_type_and_pattern_pairing_as_key_probes() {
        let sql = render(
            vec![
                EntitySelector::builder().type_expr(Some(ty("A"))).id_pattern(Some(pattern("urn:a.*"))).build(),
                EntitySelector::builder().type_expr(Some(ty("B"))).build(),
            ],
            &walked(),
        )
        .expect("constrains");
        assert_eq!(sql.matches(r#""type_member"."id" = "selected"."id""#).count(), 2, "{sql}");
        assert!(sql.contains(" OR "), "{sql}");
        assert!(!sql.contains("entity_search"), "{sql}");
    }
}
