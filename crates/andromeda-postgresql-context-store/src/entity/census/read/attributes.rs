//! The attribute answers of discovery (clauses 5.7.8 to 5.7.10), read from
//! the census ledgers.

use crate::{
    entity::{
        census::read::{Count, LiveInstances, Names, Total, live_instances, stored_count, summed, type_key_of},
        dictionary_join::{AttributeNameRow, TypeName, join_attribute_name, join_type_name},
        mapper::{stored_attribute_family, stored_attribute_name, stored_type_name},
        repo::EntityRepository,
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{attribute_summary::AttributeSummary, port::discovery::AttributeEntityTypes};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_postgresql_schema::{
    census::{AttributeCensus, EntityTypeAttributeCensus},
    name_dictionary::{AttributeNames, EntityTypeNames},
};
use andromeda_postgresql_seaquery::{aggregate::array_agg_distinct, statement::Statement};
use sea_query::{Cond, Expr, ExprTrait, Func, JoinType, Order, Query, SelectStatement};
use std::slice::from_ref;

/// Clauses 5.7.8 and 5.7.9: every attribute some Entity carries with the types
/// of the Entities carrying it, in name order.
fn attribute_entity_types_statement(target: &ContextTarget) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .column((AttributeNameRow, AttributeNames::Name))
        .expr_as(array_agg_distinct(Expr::col((TypeName, EntityTypeNames::Name))), Names)
        .from_subquery(live_instances(target, Cond::all()), LiveInstances);
    join_attribute_name(
        &mut statement,
        JoinType::InnerJoin,
        Expr::col((LiveInstances, EntityTypeAttributeCensus::AttributeNameKey)),
    );
    join_type_name(&mut statement, JoinType::InnerJoin, type_key_of(LiveInstances));
    statement
        .group_by_col((AttributeNameRow, AttributeNames::Name))
        .order_by((AttributeNameRow, AttributeNames::Name), Order::Asc);
    statement
}

/// Clause 5.7.10: the number of instances of one attribute beside every
/// (family, type) its instances have, one row per pair.
fn attribute_summary_statement(target: &ContextTarget, attribute_key: AttributeNameKey) -> SelectStatement {
    let total = Query::select()
        .expr_as(
            Func::coalesce([summed(Expr::col((AttributeCensus::Table, AttributeCensus::InstanceDelta))), Expr::val(0_i64)]),
            Count,
        )
        .from(table_of(target, AttributeCensus::Table))
        .and_where(Expr::col((AttributeCensus::Table, AttributeCensus::AttributeNameKey)).eq(attribute_key.get()))
        .take();
    let of_attribute = Cond::all().add(Expr::col((EntityTypeAttributeCensus::Table, EntityTypeAttributeCensus::AttributeNameKey)).eq(attribute_key.get()));
    let mut statement = Query::select();
    statement
        .column((Total, Count))
        .column((LiveInstances, EntityTypeAttributeCensus::AttributeKind))
        .column((TypeName, EntityTypeNames::Name))
        .from_subquery(total, Total)
        .join_subquery(JoinType::LeftJoin, live_instances(target, of_attribute), LiveInstances, Expr::val(true));
    join_type_name(&mut statement, JoinType::LeftJoin, type_key_of(LiveInstances));
    statement
}

/// Fold the rows of an attribute summary into the answer, or `None` when no
/// instance of the attribute exists.
fn attribute_summary_of(attr_iri: &AttributeName, rows: &[(i64, Option<i16>, Option<String>)]) -> Result<Option<AttributeSummary>, ContextStoreError> {
    let Some(instance_count) = rows.first().map(|(count, _, _)| *count) else {
        return Ok(None);
    };
    let instance_count = stored_count(instance_count)?;
    if instance_count == 0 {
        return Ok(None);
    }
    let mut kinds: Vec<i16> = rows.iter().filter_map(|(_, kind, _)| *kind).collect();
    kinds.sort_unstable();
    kinds.dedup();
    let mut types: Vec<&str> = rows.iter().filter_map(|(_, _, name)| name.as_deref()).collect();
    types.sort_unstable();
    types.dedup();
    Ok(Some(AttributeSummary {
        attr_iri: attr_iri.clone(), // clone: the summary owns the name it answers for.
        instance_count,
        entity_types: types.into_iter().map(stored_type_name).collect::<Result<_, _>>()?,
        attribute_types: kinds.into_iter().map(stored_attribute_family).collect::<Result<_, _>>()?,
    }))
}

impl EntityRepository {
    /// Every attribute some Entity carries with the types of the Entities
    /// carrying it, in name order (clauses 5.7.8 and 5.7.9).
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the read fails or a dictionary name
    /// no longer parses.
    pub async fn census_attribute_entity_types(&self, target: &ContextTarget) -> Result<Vec<AttributeEntityTypes>, ContextStoreError> {
        let rows: Vec<(String, Vec<String>)> = attribute_entity_types_statement(target)
            .to_query_as()
            .fetch_all(&mut **self.reads.acquire().await?)
            .await?;
        rows.into_iter()
            .map(|(attribute, types)| {
                Ok((
                    stored_attribute_name(&attribute)?,
                    types.iter().map(String::as_str).map(stored_type_name).collect::<Result<_, _>>()?,
                ))
            })
            .collect()
    }

    /// What the Entities carry of `attribute`, or `None` when no instance of it
    /// exists (clause 5.7.10).
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the dictionary lookup or the read
    /// fails, or when a stored name or family no longer reads back.
    pub async fn census_attribute_summary(&self, target: &ContextTarget, attribute: &AttributeName) -> Result<Option<AttributeSummary>, ContextStoreError> {
        let Some(attribute_key) = self.attribute_names.lookup(from_ref(attribute)).await?.get(attribute) else {
            return Ok(None);
        };
        let rows = attribute_summary_statement(target, attribute_key)
            .to_query_as()
            .fetch_all(&mut **self.reads.acquire().await?)
            .await?;
        attribute_summary_of(attribute, &rows)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::census::read::attributes::{attribute_entity_types_statement, attribute_summary_of, attribute_summary_statement};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, SelectStatement};

    fn sql(statement: &SelectStatement) -> String {
        statement.to_string(PostgresQueryBuilder)
    }

    /// A key exists while its ledger rows sum above zero, so every read keeps
    /// only the positive groups and reads no Entity row.
    #[test]
    fn every_read_keeps_the_positive_sums_and_reads_no_entity_relation() {
        let statements = [
            sql(&attribute_entity_types_statement(&ContextTarget::Live)),
            sql(&attribute_summary_statement(&ContextTarget::Live, AttributeNameKey::from(4))),
        ];
        for statement in &statements {
            assert!(statement.contains("> 0"), "{statement}");
            for relation in ["entity_search", r#""entities""#, "current_attributes"] {
                assert!(!statement.contains(relation), "{relation} read: {statement}");
            }
        }
    }

    /// An attribute's count is the type-agnostic ledger's, so an Entity with
    /// two types counts once, while its families and types come from the
    /// per-type ledger in the same statement.
    #[test]
    fn an_attribute_summary_counts_from_the_type_agnostic_ledger() {
        let statement = sql(&attribute_summary_statement(&ContextTarget::Live, AttributeNameKey::from(4)));
        assert!(statement.contains(r#"SUM("attribute_census"."instance_delta")"#), "{statement}");
        assert!(statement.contains(r#""entity_type_attribute_census"."attribute_name_key" = 4"#), "{statement}");
    }

    #[test]
    fn an_attribute_summary_deduplicates_its_families_and_types() {
        let name: AttributeName = "https://example.org/a".parse().expect("valid attribute name");
        let rows = vec![
            (6, Some(2), Some("https://example.org/U".to_owned())),
            (6, Some(1), Some("https://example.org/T".to_owned())),
            (6, Some(1), Some("https://example.org/U".to_owned())),
        ];
        let summary = attribute_summary_of(&name, &rows).expect("reads").expect("carried");
        assert_eq!(summary.instance_count, 6);
        assert_eq!(summary.attribute_types, vec![AttributeType::Property, AttributeType::Relationship]);
        let types: Vec<&str> = summary.entity_types.iter().map(EntityTypeName::as_str).collect();
        assert_eq!(types, vec!["https://example.org/T", "https://example.org/U"]);
        assert_eq!(attribute_summary_of(&name, &[(0, None, None)]).expect("reads"), None);
    }
}
