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
    statement::Statement,
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use sea_query::{Expr, ExprTrait, Iden, IntoIden, JoinType, Query, SelectStatement, TableRef, UpdateStatement};
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
fn stale_patterns_statement(current: RenderingRevision) -> SelectStatement {
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
        .to_owned()
}

/// Move the revision of `registration` on to `current`, provided it is still
/// `read`, the revision its rows were read at.
fn revision_update_statement(registration: &str, current: RenderingRevision, read: i16) -> UpdateStatement {
    Query::update()
        .table(Csr::Table)
        .value(Csr::EntityIdPatternFormsRevision, Expr::val(current.get()))
        .and_where(Expr::col(Csr::Id).eq(registration))
        .and_where(Expr::col(Csr::EntityIdPatternFormsRevision).eq(read))
        .to_owned()
}

/// The staged restatements: the two positions of a pattern row and its form.
fn rendered_columns() -> [UnnestColumn; 3] {
    [
        UnnestColumn::new(RegistrationInformation::ItemOrdinal, PostgresType::IntegerArray),
        UnnestColumn::new(RegistrationInformation::SelectorOrdinal, PostgresType::IntegerArray),
        UnnestColumn::new(RegistrationInformation::EntityIdPatternForm, PostgresType::TextArray),
    ]
}

/// Rewrite the restatement of each pattern row of `registration` at the
/// positions `item_ordinals` and `selector_ordinals` name to the form in
/// `rendered` beside them.
fn forms_update_statement(registration: &str, item_ordinals: Vec<i32>, selector_ordinals: Vec<i32>, rendered: Vec<String>) -> UnnestAliased<UpdateStatement> {
    let columns = rendered_columns();
    let source = unnest_call(
        columns
            .iter()
            .zip([Expr::val(item_ordinals), Expr::val(selector_ordinals), Expr::val(rendered)]),
    );
    let statement = Query::update()
        .table(RegistrationInformation::Table)
        .value(
            RegistrationInformation::EntityIdPatternForm,
            Expr::col((Rendered, RegistrationInformation::EntityIdPatternForm)),
        )
        .from(TableRef::FunctionCall(source, Rendered.into_iden()))
        .and_where(Expr::col((RegistrationInformation::Table, RegistrationInformation::RegistrationId)).eq(registration))
        .and_where(Expr::col((RegistrationInformation::Table, RegistrationInformation::ItemOrdinal)).equals((Rendered, RegistrationInformation::ItemOrdinal)))
        .and_where(
            Expr::col((RegistrationInformation::Table, RegistrationInformation::SelectorOrdinal)).equals((Rendered, RegistrationInformation::SelectorOrdinal)),
        )
        .to_owned();
    UnnestAliased::new(statement).with_columns(Rendered, &columns)
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
    let stale: Vec<StalePattern> = stale_patterns_statement(current).to_query_as().fetch_all(pool).await?;
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
        let moved = revision_update_statement(&first.id, current, first.entity_id_pattern_forms_revision)
            .to_query()
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        if moved == 0 {
            transaction.rollback().await?;
            continue;
        }
        forms_update_statement(&first.id, item_ordinals, selector_ordinals, rendered)
            .to_query()
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        rewritten += 1;
    }
    Ok(rewritten)
}

#[cfg(test)]
mod tests {
    use crate::{
        csource_registration::pattern_forms_refresh::{forms_update_statement, revision_update_statement, stale_patterns_statement},
        statement_text::text_of,
    };
    use andromeda_ngsi_ld_query::portable_pattern::PortablePattern;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// Only rows an older revision rendered are read: a registration this build
    /// or a newer one rendered, and one declaring no pattern (`NULL`), are not.
    #[test]
    fn only_rows_an_older_revision_rendered_are_read() {
        let (sql, values) = stale_patterns_statement(PortablePattern::RENDERING_REVISION).build(PostgresQueryBuilder);
        assert!(
            sql.contains(r#""context_source_registrations"."entity_id_pattern_forms_revision" < $1"#),
            "{sql}"
        );
        assert_eq!(values.0.len(), 1, "the revision is bound: {sql}");
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
        let (revision, values) =
            revision_update_statement("urn:ngsi-ld:ContextSourceRegistration:1", PortablePattern::RENDERING_REVISION, 0).build(PostgresQueryBuilder);
        assert!(revision.contains(r#""entity_id_pattern_forms_revision" = $1"#), "{revision}");
        assert!(revision.contains(r#""id" = $2"#), "{revision}");
        assert!(revision.contains(r#""entity_id_pattern_forms_revision" = $3"#), "{revision}");
        assert_eq!(values.0.len(), 3, "{revision}");
        assert_eq!(
            text_of(&forms_update_statement(
                "urn:ngsi-ld:ContextSourceRegistration:1",
                Vec::new(),
                Vec::new(),
                Vec::new()
            )),
            concat!(
                r#"UPDATE "registration_information" SET "entity_id_pattern_form" = "f"."entity_id_pattern_form" "#,
                r#"FROM UNNEST($1::integer[], $2::integer[], $3::text[]) AS f(item_ordinal, selector_ordinal, entity_id_pattern_form) "#,
                r#"WHERE "registration_information"."registration_id" = $4 AND "registration_information"."item_ordinal" = "f"."item_ordinal" AND "registration_information"."selector_ordinal" = "f"."selector_ordinal""#,
            )
        );
    }
}
