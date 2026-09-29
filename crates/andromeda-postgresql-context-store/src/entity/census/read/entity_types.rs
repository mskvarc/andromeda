//! The entity type answers of discovery (clauses 5.7.5 to 5.7.7), read from
//! the census ledgers.

use crate::{
    entity::{
        census::read::{
            AttributeNameRow,
            Count,
            LiveInstances,
            LiveTypes,
            Names,
            Total,
            TypeName,
            join_attribute_name,
            join_type_name,
            live_instances,
            live_types,
            stored_count,
            stored_family,
            summed,
            type_key_of,
        },
        mapper::{stored_attribute_name, stored_type_name},
        repo::EntityRepository,
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    entity_type_summary::{AttributeFamilies, EntityTypeSummary},
    port::discovery::EntityTypeAttributes,
};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use andromeda_postgresql_schema::{
    census::{EntityTypeAttributeCensus, EntityTypeCensus},
    name_dictionary::{AttributeNames, EntityTypeNames},
};
use andromeda_postgresql_seaquery::{aggregate::array_agg_distinct_filter, pg_array::text_array};
use sea_query::{Cond, Expr, ExprTrait, Func, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use std::{collections::BTreeMap, slice::from_ref};

/// Clause 5.7.5: the names of every type some Entity carries, in name order.
fn types_statement(target: &ContextTarget) -> SelectStatement {
    let mut statement = Query::select();
    statement.column((TypeName, EntityTypeNames::Name)).from_subquery(live_types(target), LiveTypes);
    join_type_name(&mut statement, JoinType::InnerJoin, type_key_of(LiveTypes));
    statement.order_by((TypeName, EntityTypeNames::Name), Order::Asc);
    statement
}

/// Clause 5.7.6: every type some Entity carries with the names of the
/// attributes its Entities carry, empty for a type whose Entities carry none.
fn entity_type_attributes_statement(target: &ContextTarget) -> SelectStatement {
    let attribute = || Expr::col((AttributeNameRow, AttributeNames::Name));
    let mut statement = Query::select();
    statement
        .column((TypeName, EntityTypeNames::Name))
        .expr_as(
            Func::coalesce([array_agg_distinct_filter(attribute(), attribute().is_not_null()), text_array(&[])]),
            Names,
        )
        .from_subquery(live_types(target), LiveTypes);
    join_type_name(&mut statement, JoinType::InnerJoin, type_key_of(LiveTypes));
    statement.join_subquery(
        JoinType::LeftJoin,
        live_instances(target, Cond::all()),
        LiveInstances,
        type_key_of(LiveInstances).eq(type_key_of(LiveTypes)),
    );
    join_attribute_name(
        &mut statement,
        JoinType::LeftJoin,
        Expr::col((LiveInstances, EntityTypeAttributeCensus::AttributeNameKey)),
    );
    statement
        .group_by_col((TypeName, EntityTypeNames::Name))
        .order_by((TypeName, EntityTypeNames::Name), Order::Asc);
    statement
}

/// Clause 5.7.7: the number of Entities carrying one type beside every
/// (attribute, family) their instances have, one row per pair, or one row with
/// no pair when the Entities carry no attribute.
fn entity_type_summary_statement(target: &ContextTarget, type_key: EntityTypeNameKey) -> SelectStatement {
    let total = Query::select()
        .expr_as(
            Func::coalesce([summed(Expr::col((EntityTypeCensus::Table, EntityTypeCensus::EntityDelta))), Expr::val(0_i64)]),
            Count,
        )
        .from(table_of(target, EntityTypeCensus::Table))
        .and_where(Expr::col((EntityTypeCensus::Table, EntityTypeCensus::EntityTypeNameKey)).eq(type_key.get()))
        .take();
    let of_type = Cond::all().add(Expr::col((EntityTypeAttributeCensus::Table, EntityTypeAttributeCensus::EntityTypeNameKey)).eq(type_key.get()));
    let mut statement = Query::select();
    statement
        .column((Total, Count))
        .column((AttributeNameRow, AttributeNames::Name))
        .column((LiveInstances, EntityTypeAttributeCensus::AttributeKind))
        .from_subquery(total, Total)
        .join_subquery(JoinType::LeftJoin, live_instances(target, of_type), LiveInstances, Expr::val(true));
    join_attribute_name(
        &mut statement,
        JoinType::LeftJoin,
        Expr::col((LiveInstances, EntityTypeAttributeCensus::AttributeNameKey)),
    );
    statement
        .order_by((AttributeNameRow, AttributeNames::Name), Order::Asc)
        .order_by((LiveInstances, EntityTypeAttributeCensus::AttributeKind), Order::Asc);
    statement
}

/// Fold the rows of a type summary into the answer, or `None` when no Entity
/// carries the type.
fn type_summary_of(rows: Vec<(i64, Option<String>, Option<i16>)>) -> Result<Option<EntityTypeSummary>, ContextStoreError> {
    let Some(entity_count) = rows.first().map(|(count, _, _)| *count) else {
        return Ok(None);
    };
    let entity_count = stored_count(entity_count)?;
    if entity_count == 0 {
        return Ok(None);
    }
    let mut attributes: BTreeMap<String, Vec<AttributeType>> = BTreeMap::new();
    for (_, name, kind) in rows {
        if let (Some(name), Some(kind)) = (name, kind) {
            attributes.entry(name).or_default().push(stored_family(kind)?);
        }
    }
    Ok(Some(EntityTypeSummary {
        entity_count,
        attributes: attributes
            .into_iter()
            .map(|(name, families)| {
                Ok(AttributeFamilies {
                    name: stored_attribute_name(&name)?,
                    families,
                })
            })
            .collect::<Result<_, ContextStoreError>>()?,
    }))
}

impl EntityRepository {
    /// Every entity type at least one Entity carries, in name order (clause 5.7.5).
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the read fails or a dictionary name
    /// is no longer a valid entity type name.
    pub async fn census_types(&self, target: &ContextTarget) -> Result<Vec<EntityTypeName>, ContextStoreError> {
        let (sql, values) = types_statement(target).build_sqlx(PostgresQueryBuilder);
        let names: Vec<String> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        names.iter().map(String::as_str).map(stored_type_name).collect()
    }

    /// Every entity type at least one Entity carries with the attributes its
    /// Entities carry, in type name order (clause 5.7.6).
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the read fails or a dictionary name
    /// no longer parses.
    pub async fn census_entity_type_attributes(&self, target: &ContextTarget) -> Result<Vec<EntityTypeAttributes>, ContextStoreError> {
        let (sql, values) = entity_type_attributes_statement(target).build_sqlx(PostgresQueryBuilder);
        let rows: Vec<(String, Vec<String>)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|(type_name, attributes)| {
                Ok((
                    stored_type_name(&type_name)?,
                    attributes.iter().map(String::as_str).map(stored_attribute_name).collect::<Result<_, _>>()?,
                ))
            })
            .collect()
    }

    /// What the Entities carrying `entity_type` hold, or `None` when none does
    /// (clause 5.7.7).
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the dictionary lookup or the read
    /// fails, or when a stored name or family no longer reads back.
    pub async fn census_entity_type_summary(
        &self,
        target: &ContextTarget,
        entity_type: &EntityTypeName,
    ) -> Result<Option<EntityTypeSummary>, ContextStoreError> {
        let Some(type_key) = self.entity_type_names.lookup(from_ref(entity_type)).await?.get(entity_type) else {
            return Ok(None);
        };
        let (sql, values) = entity_type_summary_statement(target, type_key).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        type_summary_of(rows)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::census::read::entity_types::{entity_type_attributes_statement, entity_type_summary_statement, type_summary_of, types_statement};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, SelectStatement};

    fn sql(statement: &SelectStatement) -> String {
        statement.to_string(PostgresQueryBuilder)
    }

    /// A key exists while its ledger rows sum above zero, so every read keeps
    /// only the positive groups and reads no payload or Entity row.
    #[test]
    fn every_read_keeps_the_positive_sums_and_reads_no_entity_relation() {
        let statements = [
            sql(&types_statement(&ContextTarget::Live)),
            sql(&entity_type_attributes_statement(&ContextTarget::Live)),
            sql(&entity_type_summary_statement(&ContextTarget::Live, EntityTypeNameKey::from(3))),
        ];
        for statement in &statements {
            assert!(statement.contains("> 0"), "{statement}");
            for relation in ["entity_documents", "entity_search", r#""entities""#, "current_attribute_values", "payload"] {
                assert!(!statement.contains(relation), "{relation} read: {statement}");
            }
        }
    }

    /// A type's count and its attributes come from one statement, so the
    /// answer is one snapshot.
    #[test]
    fn a_type_summary_reads_its_count_and_its_attributes_in_one_statement() {
        let statement = sql(&entity_type_summary_statement(&ContextTarget::Live, EntityTypeNameKey::from(3)));
        assert!(
            statement.contains(r#"FROM (SELECT COALESCE(SUM("entity_type_census"."entity_delta")::bigint, 0) AS "count" FROM "entity_type_census" WHERE "entity_type_census"."entity_type_name_key" = 3) AS "total""#),
            "{statement}"
        );
        assert!(statement.contains("LEFT JOIN (SELECT"), "{statement}");
        assert!(
            statement.contains(r#""entity_type_attribute_census"."entity_type_name_key" = 3"#),
            "{statement}"
        );
    }

    #[test]
    fn a_snapshot_branch_reads_its_own_census() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(5).expect("positive key");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let statement = sql(&entity_type_attributes_statement(&target));
        assert!(statement.contains(r#"FROM "snap_5_stg"."entity_type_census""#), "{statement}");
        assert!(statement.contains(r#"FROM "snap_5_stg"."entity_type_attribute_census""#), "{statement}");
        assert!(
            statement.contains(r#"JOIN "entity_type_names" AS "n""#),
            "the dictionaries are the tenant's: {statement}"
        );
    }

    #[test]
    fn a_type_no_entity_carries_has_no_summary() {
        assert_eq!(type_summary_of(Vec::new()).expect("reads"), None);
        assert_eq!(type_summary_of(vec![(0, None, None)]).expect("reads"), None);
    }

    /// The families of one attribute arrive one row each and are gathered
    /// under its name; a type whose Entities carry no attribute still has a
    /// count.
    #[test]
    fn a_type_summary_gathers_each_attributes_families() {
        let rows = vec![
            (3, Some("https://example.org/a".to_owned()), Some(1)),
            (3, Some("https://example.org/a".to_owned()), Some(2)),
            (3, Some("https://example.org/b".to_owned()), Some(3)),
        ];
        let summary = type_summary_of(rows).expect("reads").expect("carried");
        assert_eq!(summary.entity_count, 3);
        assert_eq!(summary.attributes.len(), 2);
        assert_eq!(summary.attributes[0].families, vec![AttributeType::Property, AttributeType::Relationship]);
        assert_eq!(summary.attributes[1].families, vec![AttributeType::GeoProperty]);
        let bare = type_summary_of(vec![(2, None, None)]).expect("reads").expect("carried");
        assert!(bare.attributes.is_empty(), "{bare:?}");
    }

    /// A census that summed below zero drifted from its Entities, and reading
    /// it as a count is refused rather than wrapped.
    #[test]
    fn a_negative_census_sum_is_refused() {
        assert!(type_summary_of(vec![(-1, None, None)]).is_err());
    }
}
