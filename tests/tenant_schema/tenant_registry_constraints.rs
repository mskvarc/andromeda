//! The integrity the platform tenant registry enforces on its own.
//!
//! The unnamed default tenant is the one row without a name, and only it: a
//! row whose name and default flag disagree would make the default tenant
//! ambiguous or a named tenant unaddressable, so the check refuses it however
//! the row was written.

use andromeda_test_harness::database_access::connect_test_database;
use pretty_assertions::assert_eq;

/// The SQLSTATE PostgreSQL raises for a violated check constraint.
const CHECK_VIOLATION: &str = "23514";

#[tokio::test]
async fn a_tenant_row_whose_name_and_default_flag_disagree_is_refused() {
    let pool = connect_test_database().await.expect("test database connection");

    for (name, is_unnamed_default) in [(None, false), (Some("verification-named-default"), true)] {
        let mut transaction = pool.begin().await.expect("transaction");
        let refused =
            sqlx::query("INSERT INTO tenants (status, is_unnamed_default, name, database_name) VALUES (1, $1, $2, 'andromeda_verification_never_created')")
                .bind(is_unnamed_default)
                .bind(name)
                .execute(&mut *transaction)
                .await;
        let violation = refused.expect_err("a tenant row whose name and default flag disagree must be refused");
        let code = violation.as_database_error().and_then(|error| error.code().map(std::borrow::Cow::into_owned));
        assert_eq!(code.as_deref(), Some(CHECK_VIOLATION), "the refusal must be a check violation: {violation}");
        transaction.rollback().await.expect("rollback");
    }

    pool.close().await;
}
