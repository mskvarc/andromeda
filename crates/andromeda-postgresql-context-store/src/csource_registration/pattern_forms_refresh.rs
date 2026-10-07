//! Rendering a registration's stored identifier-pattern restatements again
//! when an older rendering produced them.
//!
//! Each registration's restatements are stored with the
//! [`RenderingRevision`] that produced them. Discovery and forwarding evaluate
//! the stored text, while every other reader re-parses the registration's
//! `information` with this build's renderer, so a row an older revision
//! rendered would make the store and the broker disagree about what a
//! registered `idPattern` covers. A broker therefore renders every such row
//! again before it serves the tenant. Registrations are catalogue-scale, so this
//! is one pass over the few rows an upgrade left behind, and none at all on
//! every later open.
//!
//! Each statement stands alone, so the pass runs over the serving pool, a
//! transaction-pooling proxy included. A row a newer broker rendered is left
//! alone, and a row a concurrent registration update rewrote first is not
//! rewritten again: the update names the revision the row was read at.

use crate::{csource_registration::entity_id_pattern_forms::stored_pattern_forms, error::ContextStoreError, jsonb_text::JsonbText};
use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
use andromeda_ngsi_ld_query::portable_pattern::{PortablePattern, RenderingRevision};
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::placeholder::external_placeholder;
use sea_query::{Expr, ExprTrait, Iden, PostgresQueryBuilder, Query};
use sqlx::{PgPool, Row, types::Json};

/// The registrations whose restatements a revision older than `current`
/// rendered, with the `information` to render them from.
fn stale_rows_sql(current: RenderingRevision) -> String {
    Query::select()
        .columns([Csr::Id, Csr::Information, Csr::EntityIdPatternFormsRevision])
        .from(Csr::Table)
        .and_where(Expr::col(Csr::EntityIdPatternFormsRevision).lt(current.get()))
        .to_string(PostgresQueryBuilder)
}

/// Replace one registration's restatements and their revision, provided the
/// row still carries the revision it was read at: `$1` the registration, `$2`
/// the forms, `$3` their revision, `$4` the revision read.
fn rewrite_sql() -> String {
    Query::update()
        .table(Csr::Table)
        .values([
            (Csr::EntityIdPatternForms, external_placeholder(2)),
            (Csr::EntityIdPatternFormsRevision, external_placeholder(3)),
        ])
        .and_where(Expr::col(Csr::Id).eq(external_placeholder(1)))
        .and_where(Expr::col(Csr::EntityIdPatternFormsRevision).eq(external_placeholder(4)))
        .to_string(PostgresQueryBuilder)
}

/// Render again the identifier-pattern restatements of every registration an
/// older rendering produced, returning how many registrations were rewritten.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails, and
/// [`ContextStoreError::StoredRegistrationInformation`] when a stale row's
/// `information` no longer reads as the typed value it was written from.
pub async fn refresh_stale_pattern_forms(pool: &PgPool) -> Result<u64, ContextStoreError> {
    let current = PortablePattern::RENDERING_REVISION;
    let stale = sqlx::query(sqlx::AssertSqlSafe(stale_rows_sql(current))).fetch_all(pool).await?;
    let rewrite = rewrite_sql();
    let mut rewritten = 0;
    for row in &stale {
        let id: String = row.try_get(Csr::Id.to_string().as_str())?;
        let read_at: i16 = row.try_get(Csr::EntityIdPatternFormsRevision.to_string().as_str())?;
        let information: JsonbText<'_> = row.try_get(Csr::Information.to_string().as_str())?;
        let information: Vec<RegistrationInfo> = serde_json::from_slice(information.as_bytes()).map_err(ContextStoreError::StoredRegistrationInformation)?;
        let stored = stored_pattern_forms(&information);
        rewritten += sqlx::query(sqlx::AssertSqlSafe(rewrite.as_str()))
            .bind(&id)
            .bind(stored.as_ref().map(|stored| Json(&stored.forms)))
            .bind(stored.as_ref().map(|stored| stored.revision.get()))
            .bind(read_at)
            .execute(pool)
            .await?
            .rows_affected();
    }
    Ok(rewritten)
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::pattern_forms_refresh::{rewrite_sql, stale_rows_sql};
    use andromeda_ngsi_ld_query::portable_pattern::PortablePattern;

    /// Only rows an older revision rendered are read: a row this build or a
    /// newer one rendered, and a registration declaring no pattern (`NULL`), are
    /// not.
    #[test]
    fn only_rows_an_older_revision_rendered_are_read() {
        let sql = stale_rows_sql(PortablePattern::RENDERING_REVISION);
        assert!(
            sql.contains(&format!(
                r#""entity_id_pattern_forms_revision" < {}"#,
                PortablePattern::RENDERING_REVISION.get()
            )),
            "{sql}"
        );
        assert!(sql.contains(r#""id", "information", "entity_id_pattern_forms_revision""#), "{sql}");
    }

    /// The rewrite replaces the forms and their revision together, and only on
    /// a row still at the revision it was read at.
    #[test]
    fn the_rewrite_is_guarded_by_the_revision_it_read() {
        let sql = rewrite_sql();
        assert!(sql.contains(r#""entity_id_pattern_forms" = $2"#), "{sql}");
        assert!(sql.contains(r#""entity_id_pattern_forms_revision" = $3"#), "{sql}");
        assert!(sql.contains(r#""id" = ($1)"#), "{sql}");
        assert!(sql.contains(r#""entity_id_pattern_forms_revision" = ($4)"#), "{sql}");
    }
}
