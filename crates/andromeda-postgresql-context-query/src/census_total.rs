//! The total of a selection by one entity type alone, read from the
//! discovery census.
//!
//! The census counts the Entities whose search row carries each type, kept
//! by the statements that write the search rows in the same statement, so a
//! count is exact in every snapshot. A selection by one type with nothing else
//! narrowing it ([`EntitySelection::sole_type`]) matches the Entities of that
//! type that stand (clause 4.22): the census count less those whose
//! `expiresAt` has passed, which keep their search row until the expiry sweep
//! deletes them. The expired Entities are the few the partial index on
//! `expires_at` holds, each probed for the type on its search row by key.
//! Counting the type's members instead reads every member: a request
//! counting 500 000 Sensors that way takes 106 ms, and 1.5 ms from the
//! census.
//!
//! [`EntitySelection::sole_type`]: andromeda_ngsi_ld_query::entity_selection::EntitySelection::sole_type

use andromeda_ngsi_ld_query::{entity::compiled::CompiledEntityQuery, standing_instant::StandingInstant};
use andromeda_postgresql_query::{query_names::QueryNames, standing_instant::instant_of};
use andromeda_postgresql_schema::{
    census::EntityTypeCensus,
    current::{Entities, EntitySearch},
};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    branch::qualified,
    cast::cast,
    literal::int_literal,
    operator::array_contains_key,
    pg_type::PostgresType,
    scalar_subquery::scalar_subquery,
};
use andromeda_postgresql_stored_values::entity_type_name_key::EntityTypeNameKey;
use sea_query::{DynIden, Expr, ExprTrait, Func, Query, SelectStatement};

/// The dictionary key of the one type `query` selects every Entity of, when
/// nothing else narrows the selection and the type was ever recorded.
///
/// A type the dictionary never recorded has no Entity and no census row, and
/// its selection is answered without reading either.
#[must_use]
pub(crate) fn census_counted_type(query: &CompiledEntityQuery, names: &QueryNames) -> Option<EntityTypeNameKey> {
    query.selection.sole_type().and_then(|entity_type| names.types.get_str(entity_type.as_str()))
}

/// The number of Entities of the type keyed `entity_type` that stand at
/// `standing`, in the branch `schema` (`None` for the live branch):
///
/// ```sql
/// SELECT (SELECT COALESCE(SUM(entity_delta)::bigint, 0) FROM entity_type_census
///         WHERE entity_type_name_key = k)
///      - (SELECT COUNT(*) FROM entities JOIN entity_search USING (entity_key)
///         WHERE entities.expires_at <= <standing>
///           AND entity_search.entity_type_name_keys @> ARRAY[k])
/// ```
///
/// The key is inlined, as every type key is, so each type's count is a
/// statement of its own and the planner reads the expired Entities from the
/// statistics of their index.
#[must_use]
pub(crate) fn census_total(entity_type: EntityTypeNameKey, schema: Option<&DynIden>, standing: StandingInstant) -> SelectStatement {
    let counted = Query::select()
        .expr(Func::coalesce([
            cast(Func::sum(Expr::col(EntityTypeCensus::EntityDelta)).into(), PostgresType::Bigint),
            Expr::val(0_i64),
        ]))
        .from(qualified(schema, EntityTypeCensus::Table))
        .and_where(Expr::col(EntityTypeCensus::EntityTypeNameKey).eq(int_literal(i64::from(entity_type.get()))))
        .take();
    let expired = Query::select()
        .expr(count_all())
        .from(qualified(schema, Entities::Table))
        .inner_join(
            qualified(schema, EntitySearch::Table),
            Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
        )
        .and_where(Expr::col((Entities::Table, Entities::ExpiresAt)).lte(instant_of(standing)))
        .and_where(array_contains_key(
            Expr::col((EntitySearch::Table, EntitySearch::EntityTypeNameKeys)),
            entity_type.get(),
        ))
        .take();
    Query::select().expr(scalar_subquery(counted).sub(scalar_subquery(expired))).take()
}

#[cfg(test)]
mod tests {
    use crate::census_total::census_total;
    use andromeda_ngsi_ld_query::standing_instant::StandingInstant;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::entity_type_name_key::EntityTypeNameKey;
    use sea_query::{Alias, DynIden, IntoIden};

    #[test]
    fn a_type_total_is_its_census_count_less_its_expired_entities() {
        assert_statement_snapshot!(
            "census-total-a-type-total-is-its-census-count-less-its-expired-entities",
            census_total(EntityTypeNameKey::from(3), None, StandingInstant::WhenRead)
        );
    }

    #[test]
    fn a_snapshot_branch_counts_from_its_own_census_and_entities() {
        let schema: DynIden = Alias::new("snap_5").into_iden();
        assert_statement_snapshot!(
            "census-total-a-snapshot-branch-counts-from-its-own-census-and-entities",
            census_total(EntityTypeNameKey::from(3), Some(&schema), StandingInstant::WhenRead)
        );
    }
}
