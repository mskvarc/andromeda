//! Registration routing plans: discovery stays sublinear in the number of
//! registrations.
//!
//! `registration_information` stores an absent axis as `NULL`, and its
//! indexes spell it as a sentinel (`coalesce(entity_types, '{*}')` on the type
//! axis), so each discovery axis is a single overlap or membership test over
//! the indexed expression. This is what makes discovery sublinear in the
//! number of registrations: the semi-join is driven from the index to the few
//! registrations declaring the requested value (or declaring everything),
//! instead of probing the rows once per registration.

use crate::seeded_corpus::{SEED_PREFIX, explain};
use andromeda_postgresql_codes::csr_mode::CsrModeCode;
use andromeda_postgresql_schema::csr::{Csr, RegistrationInformation};
use andromeda_postgresql_seaquery::{
    cast::{cast, typed_null},
    catalog_function::generate_series,
    command::Command,
    literal::{false_literal, int_literal, text_literal},
    maintenance::{MaintenanceSchema, analyze},
    pg_array::{array_literal, text_array},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    statement::Statement,
};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use sea_query::{Alias, Expr, ExprTrait, Func, IntoIden, Query, SelectStatement, SimpleExpr};
use sqlx::PgPool;

/// How many selective registrations the corpus holds beside the rare one and
/// the wildcard one.
const COMMON_REGISTRATIONS: i32 = 2_000;

/// The expanded type every common registration declares.
const COMMON_TYPE: &str = "https://example.test/Common";

/// The expanded type exactly one registration declares.
const RARE_TYPE: &str = "https://example.test/Rare";

/// Seed `COMMON_REGISTRATIONS` type-only registrations declaring the common
/// type, one declaring the rare type, and one whose only item names an
/// attribute and no entities, each with the information rows the broker would
/// write.
async fn seed_registrations(pool: &PgPool, prefix: &str) {
    seed_common_registrations(pool, prefix).await;
    for (label, entity_types, property_name_keys) in [("rare", Some(vec![RARE_TYPE]), None), ("wildcard", None, Some(vec![1]))] {
        seed_labelled_registration(pool, prefix, label, entity_types, property_name_keys).await;
    }

    for relation in [Csr::Table.into_iden(), RegistrationInformation::Table.into_iden()] {
        analyze(MaintenanceSchema::Public, relation)
            .to_query()
            .execute(pool)
            .await
            .expect("the seeded relations must be analysed");
    }
}

/// Seed the `COMMON_REGISTRATIONS` type-only registrations and their
/// information rows.
async fn seed_common_registrations(pool: &PgPool, prefix: &str) {
    let common_id = || {
        cast(Expr::val(prefix), PostgresType::Text)
            .concat_text(text_literal(":ContextSourceRegistration:common-"))
            .concat_text(Expr::col(Alias::new("n")))
    };
    Query::insert()
        .into_table(Csr::Table)
        .columns([Csr::Id, Csr::Endpoint, Csr::Mode, Csr::IsManagementDeclared])
        .select_from(
            Query::select()
                .expr(common_id())
                .expr(Expr::val("http://localhost:9/csource"))
                .expr(Expr::val(CsrModeCode::Inclusive.code()))
                .expr(false_literal())
                .from_function(generate_series(Expr::val(1), Expr::val(COMMON_REGISTRATIONS)), Alias::new("n"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("the common registrations must be seeded");
    Query::insert()
        .into_table(RegistrationInformation::Table)
        .columns([
            RegistrationInformation::RegistrationId,
            RegistrationInformation::ItemOrdinal,
            RegistrationInformation::SelectorOrdinal,
            RegistrationInformation::EntityTypes,
            RegistrationInformation::EntityTypeShape,
        ])
        .select_from(
            Query::select()
                .expr(common_id())
                .expr(Expr::val(0))
                .expr(Expr::val(0))
                .expr(array_literal([cast(Expr::val(COMMON_TYPE), PostgresType::Text)]))
                .expr(Expr::val(1))
                .from_function(generate_series(Expr::val(1), Expr::val(COMMON_REGISTRATIONS)), Alias::new("n"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("the common information rows must be seeded");
}

/// Seed one registration `label` with its single information row, declaring
/// `entity_types` or `property_name_keys`.
async fn seed_labelled_registration(pool: &PgPool, prefix: &str, label: &str, entity_types: Option<Vec<&str>>, property_name_keys: Option<Vec<i32>>) {
    let registration_id = || {
        cast(Expr::val(prefix), PostgresType::Text)
            .concat_text(text_literal(":ContextSourceRegistration:"))
            .concat_text(Expr::val(label))
    };
    Query::insert()
        .into_table(Csr::Table)
        .columns([Csr::Id, Csr::Endpoint, Csr::Mode, Csr::IsManagementDeclared])
        .values([
            registration_id(),
            Expr::val("http://localhost:9/csource"),
            Expr::val(CsrModeCode::Inclusive.code()),
            false_literal(),
        ])
        .expect("one value per column")
        .to_query()
        .execute(pool)
        .await
        .expect("the registration must be seeded");
    let shape_cell = if entity_types.is_some() {
        Expr::val(1_i16)
    } else {
        typed_null(PostgresType::Smallint)
    };
    let entity_types_cell = entity_types.map_or_else(
        || typed_null(PostgresType::TextArray),
        |types| Expr::val(types.into_iter().map(str::to_owned).collect::<Vec<String>>()),
    );
    let property_cell = property_name_keys.map_or_else(|| typed_null(PostgresType::IntegerArray), Expr::val);
    Query::insert()
        .into_table(RegistrationInformation::Table)
        .columns([
            RegistrationInformation::RegistrationId,
            RegistrationInformation::ItemOrdinal,
            RegistrationInformation::SelectorOrdinal,
            RegistrationInformation::EntityTypes,
            RegistrationInformation::EntityTypeShape,
            RegistrationInformation::PropertyNameKeys,
        ])
        .values([registration_id(), Expr::val(0), Expr::val(0), entity_types_cell, shape_cell, property_cell])
        .expect("one value per column")
        .to_query()
        .execute(pool)
        .await
        .expect("the information row must be seeded");
}

/// The discovery preselect for one type as the broker renders it: one overlap
/// of the indexed expression with the requested type beside the wildcard
/// sentinel.
fn type_preselect(entity_type: &str) -> SelectStatement {
    Query::select()
        .column((Alias::new("csr"), Csr::Id))
        .from_as(Csr::Table, Alias::new("csr"))
        .and_where(Expr::exists(
            Query::select()
                .expr(int_literal(1))
                .from_as(RegistrationInformation::Table, Alias::new("ri"))
                .and_where(Expr::col((Alias::new("ri"), RegistrationInformation::RegistrationId)).equals((Alias::new("csr"), Csr::Id)))
                .and_where(
                    SimpleExpr::from(Func::coalesce([
                        Expr::col((Alias::new("ri"), RegistrationInformation::EntityTypes)),
                        cast(text_literal("{*}"), PostgresType::TextArray),
                    ]))
                    .array_overlaps(text_array(&[entity_type, "*"])),
                )
                .to_owned(),
        ))
        .to_owned()
}

#[tokio::test]
async fn a_selective_type_preselect_is_driven_from_the_type_index_not_a_registration_scan() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed_registrations(pool, SEED_PREFIX).await;

    let plan = explain(pool, type_preselect(RARE_TYPE)).await;
    assert!(
        plan.contains("Bitmap Index Scan on idx_registration_information_entity_types"),
        "the type overlap must be answered by the expression's GIN index: {plan}"
    );
    assert!(
        !plan.contains("Seq Scan on registration_information"),
        "the information rows must not be scanned once per registration: {plan}"
    );

    let prefix = SEED_PREFIX;
    let matched: Vec<String> = type_preselect(RARE_TYPE)
        .and_where(Expr::col((Alias::new("csr"), Csr::Id)).like(format!("{prefix}%")))
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the preselect must run");
    let mut matched = matched;
    matched.sort();
    assert_eq!(
        matched,
        vec![
            format!("{prefix}:ContextSourceRegistration:rare"),
            format!("{prefix}:ContextSourceRegistration:wildcard")
        ],
        "the rare type admits exactly the declaring registration and the unconstrained one"
    );

    scope.finish().await.expect("cleanup failed");
}
