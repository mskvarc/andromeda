//! Registration routing plans: discovery stays sublinear in the number of
//! registrations.
//!
//! The routing projection stores an axis a row does not constrain as a
//! sentinel rather than as `NULL`, and a registration constraining nothing as
//! one wildcard row, so that each discovery dimension is a single overlap or
//! membership test the projection's own index answers. This is what makes
//! discovery sublinear in the number of registrations: the semi-join is driven
//! from the projection's index to the few registrations declaring the requested
//! value (or declaring everything), instead of probing the projection once per
//! registration to test for the absence of rows.

use crate::seeded_corpus::{SEED_PREFIX, explain};
use andromeda_postgresql_codes::csr_mode::CsrModeCode;
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use sqlx::PgPool;

/// How many selective registrations the corpus holds beside the rare one and
/// the wildcard one.
const COMMON_REGISTRATIONS: i32 = 2_000;

/// The expanded type every common registration declares.
const COMMON_TYPE: &str = "https://example.test/Common";

/// The expanded type exactly one registration declares.
const RARE_TYPE: &str = "https://example.test/Rare";

/// Seed `COMMON_REGISTRATIONS` type-only registrations declaring the common
/// type, one declaring the rare type, and one constraining nothing, each with
/// the routing rows the broker would derive.
async fn seed_registrations(pool: &PgPool, prefix: &str) {
    sqlx::query(
        "INSERT INTO context_source_registrations (id, information, endpoint, mode) \
         SELECT $1 || ':ContextSourceRegistration:common-' || n, \
                jsonb_build_array(jsonb_build_object('entities', jsonb_build_array(jsonb_build_object('type', $2::text)))), \
                'http://localhost:9/csource', $4 \
         FROM generate_series(1, $3) AS n",
    )
    .bind(prefix)
    .bind(COMMON_TYPE)
    .bind(COMMON_REGISTRATIONS)
    .bind(CsrModeCode::Inclusive.code())
    .execute(pool)
    .await
    .expect("the common registrations must be seeded");
    sqlx::query(
        "INSERT INTO registration_information (registration_id, entity_types, entity_id, property_name_keys, relationship_name_keys) \
         SELECT $1 || ':ContextSourceRegistration:common-' || n, ARRAY[$2::text], '', ARRAY[]::integer[], ARRAY[]::integer[] \
         FROM generate_series(1, $3) AS n",
    )
    .bind(prefix)
    .bind(COMMON_TYPE)
    .bind(COMMON_REGISTRATIONS)
    .execute(pool)
    .await
    .expect("the common routing rows must be seeded");

    for (label, entity_types) in [("rare", vec![RARE_TYPE]), ("wildcard", vec!["*"])] {
        sqlx::query(
            "INSERT INTO context_source_registrations (id, information, endpoint, mode) \
             VALUES ($1 || ':ContextSourceRegistration:' || $2, '[]'::jsonb, 'http://localhost:9/csource', $3)",
        )
        .bind(prefix)
        .bind(label)
        .bind(CsrModeCode::Inclusive.code())
        .execute(pool)
        .await
        .expect("the registration must be seeded");
        sqlx::query(
            "INSERT INTO registration_information (registration_id, entity_types, entity_id, property_name_keys, relationship_name_keys) \
             VALUES ($1 || ':ContextSourceRegistration:' || $2, $3::text[], '', ARRAY[0], ARRAY[0])",
        )
        .bind(prefix)
        .bind(label)
        .bind(&entity_types)
        .execute(pool)
        .await
        .expect("the routing row must be seeded");
    }

    sqlx::query("ANALYZE context_source_registrations, registration_information")
        .execute(pool)
        .await
        .expect("the seeded relations must be analysed");
}

/// The discovery preselect for one type as the broker renders it: one overlap
/// naming the requested type beside the wildcard sentinel.
fn type_preselect(entity_type: &str) -> String {
    format!(
        "SELECT csr.id FROM context_source_registrations AS csr \
         WHERE EXISTS (SELECT 1 FROM registration_information AS ri \
                       WHERE ri.registration_id = csr.id AND ri.entity_types && ARRAY['{entity_type}', '*'])"
    )
}

#[tokio::test]
async fn a_selective_type_preselect_is_driven_from_the_projection_index_not_a_registration_scan() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed_registrations(pool, SEED_PREFIX).await;

    let plan = explain(pool, &type_preselect(RARE_TYPE)).await;
    assert!(
        plan.contains("Bitmap Index Scan on idx_registration_information_entity_types"),
        "the type overlap must be answered by the projection's GIN index: {plan}"
    );
    assert!(
        !plan.contains("Seq Scan on registration_information"),
        "the projection must not be scanned once per registration: {plan}"
    );

    let prefix = SEED_PREFIX;
    let matched: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("{} AND csr.id LIKE '{prefix}%'", type_preselect(RARE_TYPE))))
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
