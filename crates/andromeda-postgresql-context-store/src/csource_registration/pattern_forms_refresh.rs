//! Rendering a registration's stored identifier-pattern restatements again
//! when an older rendering produced them.
//!
//! Each registration records the [`RenderingRevision`] that produced the
//! restatements on its information rows. Discovery and forwarding evaluate the
//! stored restatement, while every other reader parses the pattern as written
//! with this build's renderer, so a row an older revision rendered would make
//! the store and the broker disagree about what a registered `idPattern`
//! covers. A broker therefore renders every such row again before it serves
//! the tenant. Registrations are catalogue-scale, so this is one pass over the
//! few rows an upgrade left behind, and none at all on every later open.
//!
//! Each registration is rewritten in a transaction of its own, so the pass runs
//! over the serving pool, a transaction-pooling proxy included. A registration
//! a newer broker rendered is left alone, and one a concurrent update rewrote
//! first is not rewritten again: the revision update names the revision the
//! rows were read at, and the rows are rewritten only when it matched.

use crate::{
    csource_registration::{stored_information_error::StoredInformationError, stored_registration_error::StoredRegistrationError},
    error::ContextStoreError,
};
use andromeda_ngsi_ld_query::{
    pattern::Pattern,
    portable_pattern::{PortablePattern, RenderingRevision},
};
use andromeda_postgresql_schema::csr::{Csr, RegistrationInformation};
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{Expr, ExprTrait, Iden, IntoIden, JoinType, PostgresQueryBuilder, Query, TableRef};
use sqlx::PgPool;

/// The `UNNEST` alias the rewritten restatements are expanded under.
#[derive(Iden, Clone, Copy)]
#[iden = "f"]
struct Rendered;

/// One stored pattern of a registration an older revision rendered.
#[derive(Debug, sqlx::FromRow)]
struct StalePattern {
    /// The registration.
    id: String,
    /// The revision its rows were read at.
    entity_id_pattern_forms_revision: i16,
    /// The pattern's item position.
    item_ordinal: i32,
    /// The pattern's selector position.
    selector_ordinal: i32,
    /// The pattern as written.
    entity_id_pattern: String,
}

/// The alias of one stored pattern's information row, `i`.
#[derive(Iden, Clone, Copy)]
#[iden = "i"]
struct StoredPattern;

/// Every stored pattern of the registrations a revision older than `current`
/// rendered, grouped by registration.
fn stale_patterns_sql(current: RenderingRevision) -> String {
    Query::select()
        .column((Csr::Table, Csr::Id))
        .column((Csr::Table, Csr::EntityIdPatternFormsRevision))
        .column((StoredPattern, RegistrationInformation::ItemOrdinal))
        .column((StoredPattern, RegistrationInformation::SelectorOrdinal))
        .column((StoredPattern, RegistrationInformation::EntityIdPattern))
        .from(Csr::Table)
        .join_as(
            JoinType::InnerJoin,
            RegistrationInformation::Table,
            StoredPattern,
            Expr::col((StoredPattern, RegistrationInformation::RegistrationId)).equals((Csr::Table, Csr::Id)),
        )
        .and_where(Expr::col((Csr::Table, Csr::EntityIdPatternFormsRevision)).lt(current.get()))
        .and_where(Expr::col((StoredPattern, RegistrationInformation::EntityIdPattern)).is_not_null())
        .order_by((Csr::Table, Csr::Id), sea_query::Order::Asc)
        .to_string(PostgresQueryBuilder)
}

/// Move one registration's revision on, provided it is still the revision its
/// rows were read at: `$1` the registration, `$2` the new revision, `$3` the
/// revision read.
fn revision_update_sql() -> String {
    Query::update()
        .table(Csr::Table)
        .value(Csr::EntityIdPatternFormsRevision, external_placeholder(2))
        .and_where(Expr::col(Csr::Id).eq(external_placeholder(1)))
        .and_where(Expr::col(Csr::EntityIdPatternFormsRevision).eq(external_placeholder(3)))
        .to_string(PostgresQueryBuilder)
}

/// The staged restatements, `$2` to `$4`.
fn rendered_columns() -> [UnnestColumn; 3] {
    [
        UnnestColumn::new(RegistrationInformation::ItemOrdinal, PostgresType::IntegerArray),
        UnnestColumn::new(RegistrationInformation::SelectorOrdinal, PostgresType::IntegerArray),
        UnnestColumn::new(RegistrationInformation::EntityIdPatternForm, PostgresType::TextArray),
    ]
}

/// Rewrite the restatement of each pattern row of the registration `$1` at
/// the positions `$2` and `$3` to `$4`.
fn forms_update_sql() -> String {
    let columns = rendered_columns();
    let source = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 2))));
    let sql = Query::update()
        .table(RegistrationInformation::Table)
        .value(
            RegistrationInformation::EntityIdPatternForm,
            Expr::col((Rendered, RegistrationInformation::EntityIdPatternForm)),
        )
        .from(TableRef::FunctionCall(source, Rendered.into_iden()))
        .and_where(Expr::col((RegistrationInformation::Table, RegistrationInformation::RegistrationId)).eq(external_placeholder(1)))
        .and_where(Expr::col((RegistrationInformation::Table, RegistrationInformation::ItemOrdinal)).equals((Rendered, RegistrationInformation::ItemOrdinal)))
        .and_where(
            Expr::col((RegistrationInformation::Table, RegistrationInformation::SelectorOrdinal)).equals((Rendered, RegistrationInformation::SelectorOrdinal)),
        )
        .to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &Rendered, &columns)
}

/// Render again the identifier-pattern restatements of every registration an
/// older rendering produced, returning how many registrations were rewritten.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails, and
/// [`ContextStoreError::StoredRegistration`] when a stored pattern no longer
/// compiles.
pub async fn refresh_stale_pattern_forms(pool: &PgPool) -> Result<u64, ContextStoreError> {
    let current = PortablePattern::RENDERING_REVISION;
    let stale: Vec<StalePattern> = sqlx::query_as(sqlx::AssertSqlSafe(stale_patterns_sql(current))).fetch_all(pool).await?;
    let revision_update = revision_update_sql();
    let forms_update = forms_update_sql();
    let mut rewritten = 0;
    for patterns in stale.chunk_by(|left, right| left.id == right.id) {
        let Some(first) = patterns.first() else {
            continue;
        };
        let rendered = patterns
            .iter()
            .map(|pattern| Pattern::parse(&pattern.entity_id_pattern).map(|parsed| parsed.portable().as_str().to_owned()))
            .collect::<Result<Vec<String>, _>>()
            .map_err(|error| StoredRegistrationError::Information(StoredInformationError::IdPattern(Box::new(error))))?;
        let item_ordinals: Vec<i32> = patterns.iter().map(|pattern| pattern.item_ordinal).collect();
        let selector_ordinals: Vec<i32> = patterns.iter().map(|pattern| pattern.selector_ordinal).collect();
        let mut transaction = pool.begin().await?;
        let moved = sqlx::query(sqlx::AssertSqlSafe(revision_update.as_str()))
            .bind(&first.id)
            .bind(current.get())
            .bind(first.entity_id_pattern_forms_revision)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        if moved == 0 {
            transaction.rollback().await?;
            continue;
        }
        sqlx::query(sqlx::AssertSqlSafe(forms_update.as_str()))
            .bind(&first.id)
            .bind(&item_ordinals)
            .bind(&selector_ordinals)
            .bind(&rendered)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        rewritten += 1;
    }
    Ok(rewritten)
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::pattern_forms_refresh::{forms_update_sql, revision_update_sql, stale_patterns_sql};
    use andromeda_ngsi_ld_query::portable_pattern::PortablePattern;
    use pretty_assertions::assert_eq;

    /// Only rows an older revision rendered are read: a registration this build
    /// or a newer one rendered, and one declaring no pattern (`NULL`), are not.
    #[test]
    fn only_rows_an_older_revision_rendered_are_read() {
        let sql = stale_patterns_sql(PortablePattern::RENDERING_REVISION);
        assert!(
            sql.contains(&format!(
                r#""context_source_registrations"."entity_id_pattern_forms_revision" < {}"#,
                PortablePattern::RENDERING_REVISION.get()
            )),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "registration_information" AS "i" ON "i"."registration_id" = "context_source_registrations"."id""#),
            "{sql}"
        );
        assert!(sql.contains(r#""i"."entity_id_pattern" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#""i"."item_ordinal", "i"."selector_ordinal", "i"."entity_id_pattern""#), "{sql}");
    }

    /// The revision moves on only on a registration still at the revision its
    /// rows were read at, and the restatements are written to
    /// `registration_information.entity_id_pattern_form` by position.
    #[test]
    fn the_rewrite_is_guarded_by_the_revision_it_read() {
        let revision = revision_update_sql();
        assert!(revision.contains(r#""entity_id_pattern_forms_revision" = $2"#), "{revision}");
        assert!(revision.contains(r#""id" = ($1)"#), "{revision}");
        assert!(revision.contains(r#""entity_id_pattern_forms_revision" = ($3)"#), "{revision}");
        assert_eq!(
            forms_update_sql(),
            concat!(
                r#"UPDATE "registration_information" SET "entity_id_pattern_form" = "f"."entity_id_pattern_form" "#,
                r#"FROM UNNEST($2::integer[], $3::integer[], $4::text[]) AS f(item_ordinal, selector_ordinal, entity_id_pattern_form) "#,
                r#"WHERE "registration_information"."registration_id" = ($1) AND "registration_information"."item_ordinal" = "f"."item_ordinal" AND "registration_information"."selector_ordinal" = "f"."selector_ordinal""#,
            )
        );
    }
}
