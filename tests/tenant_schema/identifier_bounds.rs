//! Identifier storage bounds: every identifier column refuses a value past the
//! identifier octet bound in the database itself.
//!
//! Every identifier column is held to the identifier bound the parsers refuse
//! longer input with (`IDENTIFIER_BYTE_LIMIT`), measured in UTF-8 octets
//! rather than characters because octets are what the B-tree and trigram
//! indexes over those columns have to hold. The parser is the first line; the
//! named check constraint is the one that still holds when a row arrives by a
//! path that skipped it. A raw statement therefore stands in for such a path:
//! one octet past the bound is refused with a check violation naming the
//! column's own constraint, and exactly the bound is stored.

use crate::SUITE;
use andromeda_ngsi_ld_lexicon::identifier_length::IDENTIFIER_BYTE_LIMIT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sqlx::PgPool;
use std::{borrow::Cow, iter};

/// The SQLSTATE PostgreSQL raises for a violated check constraint.
const CHECK_VIOLATION: &str = "23514";

/// One identifier column, the constraint bounding it, and a statement that
/// writes a row whose only unusual member is that column, bound as `$1`.
struct BoundedColumn {
    /// The `relation.column` the statement writes, for failure messages.
    column: &'static str,
    /// The check constraint the refusal must name.
    constraint: &'static str,
    /// An insert that is valid in every respect except the bound value.
    insert: &'static str,
}

/// The identifier columns a raw insert can reach without first creating the
/// rows they reference.
const BOUNDED_COLUMNS: &[BoundedColumn] = &[
    BoundedColumn {
        column: "entities.id",
        constraint: "chk_entities_id_length",
        insert: "INSERT INTO entities (id) VALUES ($1)",
    },
    BoundedColumn {
        column: "temporal_entities.id",
        constraint: "chk_temporal_entities_id_length",
        insert: "INSERT INTO temporal_entities (created_at, modified_at, id, entity_type_name_keys) VALUES (now(), now(), $1, ARRAY[1])",
    },
    BoundedColumn {
        column: "attribute_names.name",
        constraint: "chk_attribute_names_name_length",
        insert: "INSERT INTO attribute_names (name) VALUES ($1)",
    },
    BoundedColumn {
        column: "entity_type_names.name",
        constraint: "chk_entity_type_names_name_length",
        insert: "INSERT INTO entity_type_names (name) VALUES ($1)",
    },
    BoundedColumn {
        column: "jsonld_contexts.id",
        constraint: "chk_jsonld_contexts_id_length",
        insert: "INSERT INTO jsonld_contexts (kind, id, payload) VALUES (2, $1, '{}'::json)",
    },
    BoundedColumn {
        column: "entity_maps.id",
        constraint: "chk_entity_maps_id_length",
        insert: "INSERT INTO entity_maps (expires_at, id) VALUES (now() + interval '1 hour', $1)",
    },
    BoundedColumn {
        column: "snapshots.id",
        constraint: "chk_snapshots_id_length",
        insert: "INSERT INTO snapshots (expires_at, id, query_context) VALUES (now() + interval '1 hour', $1, '{}'::json)",
    },
];

/// An identifier of exactly `octets` UTF-8 octets that starts with `prefix`,
/// padded with `padding`, which must divide the remainder evenly.
fn identifier_of_octets(prefix: &str, padding: char, octets: usize) -> String {
    let remainder = octets.checked_sub(prefix.len()).expect("the prefix fits inside the requested length");
    assert_eq!(remainder % padding.len_utf8(), 0, "the padding fills the remainder exactly");
    let identifier: String = prefix.chars().chain(iter::repeat_n(padding, remainder / padding.len_utf8())).collect();
    assert_eq!(identifier.len(), octets, "the identifier has the requested width");
    identifier
}

/// Run `insert` with `value` inside a transaction that is always rolled back,
/// and return the SQLSTATE and the constraint the database refused it with.
async fn refusal(pool: &PgPool, insert: &'static str, value: &str) -> (Option<String>, Option<String>) {
    let mut transaction = pool.begin().await.expect("transaction");
    let outcome = sqlx::query(insert).bind(value).execute(&mut *transaction).await;
    transaction.rollback().await.expect("rollback");
    let violation = outcome.expect_err("a value one octet past the identifier bound must be refused");
    let database_error = violation.as_database_error().expect("the refusal comes from the database");
    (database_error.code().map(Cow::into_owned), database_error.constraint().map(str::to_owned))
}

#[tokio::test]
async fn every_identifier_column_refuses_one_byte_past_the_limit_at_the_database() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let prefix = scope.id("IdentifierBound:");

    let oversized = identifier_of_octets(&prefix, 'x', IDENTIFIER_BYTE_LIMIT + 1);
    for bounded in BOUNDED_COLUMNS {
        let (code, constraint) = refusal(pool, bounded.insert, &oversized).await;
        assert_eq!(
            code.as_deref(),
            Some(CHECK_VIOLATION),
            "{} must refuse an oversized value with a check violation",
            bounded.column
        );
        assert_eq!(
            constraint.as_deref(),
            Some(bounded.constraint),
            "{} must be refused by its own bound",
            bounded.column
        );
    }

    // The bound counts octets, not characters: a value of fewer than
    // IDENTIFIER_BYTE_LIMIT characters is still refused once its two-octet
    // characters carry it past the bound.
    let wide_prefix: Cow<'_, str> = if prefix.len() % 2 == IDENTIFIER_BYTE_LIMIT % 2 {
        Cow::Owned(format!("{prefix}x"))
    } else {
        Cow::Borrowed(&prefix)
    };
    let multibyte = identifier_of_octets(&wide_prefix, '\u{e9}', IDENTIFIER_BYTE_LIMIT + 1);
    assert!(
        multibyte.chars().count() < IDENTIFIER_BYTE_LIMIT,
        "the multibyte value is shorter in characters than the bound"
    );
    let (code, constraint) = refusal(pool, "INSERT INTO entities (id) VALUES ($1)", &multibyte).await;
    assert_eq!(code.as_deref(), Some(CHECK_VIOLATION), "an identifier over the bound in octets is refused");
    assert_eq!(constraint.as_deref(), Some("chk_entities_id_length"));

    // Exactly the bound is storable, so the check pins the identifier bound
    // rather than an arbitrary smaller one. The row is written and read back
    // inside a transaction that is rolled back, so it never outlives the test.
    let widest = identifier_of_octets(&prefix, 'x', IDENTIFIER_BYTE_LIMIT);
    let mut transaction = pool.begin().await.expect("transaction");
    let stored: i32 = sqlx::query_scalar("INSERT INTO entities (id) VALUES ($1) RETURNING octet_length(id)")
        .bind(&widest)
        .fetch_one(&mut *transaction)
        .await
        .expect("an identifier of exactly the bound must be storable");
    transaction.rollback().await.expect("rollback");
    assert_eq!(usize::try_from(stored).expect("an octet length is non-negative"), IDENTIFIER_BYTE_LIMIT);

    let leaked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entities WHERE id = $1")
        .bind(&widest)
        .fetch_one(pool)
        .await
        .expect("the entity relation must be readable");
    assert_eq!(leaked, 0, "the widest identifier was rolled back with its transaction");

    scope.finish().await.expect("cleanup failed");
}
