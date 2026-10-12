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
/// type, one declaring the rare type, and one whose only item names an
/// attribute and no entities, each with the information rows the broker would
/// write.
async fn seed_registrations(pool: &PgPool, prefix: &str) {
    sqlx::query(
        "INSERT INTO context_source_registrations (id, endpoint, mode, is_management_declared) \
         SELECT $1 || ':ContextSourceRegistration:common-' || n, 'http://localhost:9/csource', $3, false \
         FROM generate_series(1, $2) AS n",
    )
    .bind(prefix)
    .bind(COMMON_REGISTRATIONS)
    .bind(CsrModeCode::Inclusive.code())
    .execute(pool)
    .await
    .expect("the common registrations must be seeded");
    sqlx::query(
        "INSERT INTO registration_information (registration_id, item_ordinal, selector_ordinal, entity_types, entity_type_shape) \
         SELECT $1 || ':ContextSourceRegistration:common-' || n, 0, 0, ARRAY[$2::text], 1 \
         FROM generate_series(1, $3) AS n",
    )
    .bind(prefix)
    .bind(COMMON_TYPE)
    .bind(COMMON_REGISTRATIONS)
    .execute(pool)
    .await
    .expect("the common information rows must be seeded");

    for (label, entity_types, property_name_keys) in [("rare", Some(vec![RARE_TYPE]), None), ("wildcard", None, Some(vec![1]))] {
        sqlx::query(
            "INSERT INTO context_source_registrations (id, endpoint, mode, is_management_declared) \
             VALUES ($1 || ':ContextSourceRegistration:' || $2, 'http://localhost:9/csource', $3, false)",
        )
        .bind(prefix)
        .bind(label)
        .bind(CsrModeCode::Inclusive.code())
        .execute(pool)
        .await
        .expect("the registration must be seeded");
        sqlx::query(
            "INSERT INTO registration_information (registration_id, item_ordinal, selector_ordinal, entity_types, entity_type_shape, property_name_keys) \
             VALUES ($1 || ':ContextSourceRegistration:' || $2, 0, 0, $3::text[], CASE WHEN $3::text[] IS NULL THEN NULL ELSE 1 END, $4::integer[])",
        )
        .bind(prefix)
        .bind(label)
        .bind(&entity_types)
        .bind(&property_name_keys)
        .execute(pool)
        .await
        .expect("the information row must be seeded");
    }

    sqlx::query("ANALYZE context_source_registrations, registration_information")
        .execute(pool)
        .await
        .expect("the seeded relations must be analysed");
}

/// The discovery preselect for one type as the broker renders it: one overlap
/// of the indexed expression with the requested type beside the wildcard
/// sentinel.
fn type_preselect(entity_type: &str) -> String {
    format!(
        "SELECT csr.id FROM context_source_registrations AS csr \
         WHERE EXISTS (SELECT 1 FROM registration_information AS ri \
                       WHERE ri.registration_id = csr.id AND coalesce(ri.entity_types, '{{*}}') && ARRAY['{entity_type}', '*'])"
    )
}

#[tokio::test]
async fn a_selective_type_preselect_is_driven_from_the_type_index_not_a_registration_scan() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed_registrations(pool, SEED_PREFIX).await;

    let plan = explain(pool, &type_preselect(RARE_TYPE)).await;
    assert!(
        plan.contains("Bitmap Index Scan on idx_registration_information_entity_types"),
        "the type overlap must be answered by the expression's GIN index: {plan}"
    );
    assert!(
        !plan.contains("Seq Scan on registration_information"),
        "the information rows must not be scanned once per registration: {plan}"
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
