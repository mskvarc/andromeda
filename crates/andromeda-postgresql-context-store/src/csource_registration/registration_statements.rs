//! Every read selects through the one `registration_select` projection that discovery shares; this module owns the writes.

use crate::{
    csource_registration::{mode_code::stored_mode, operation_code::stored_operation, status_code::stored_status},
    store_fault::persistence_error,
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, scope::path::ScopePath};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    registration::{forwarding_mode::ForwardingMode, operation::CsrOperation, status::CsrStatus},
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_postgresql_query::ltree::StoredScopePath;
use andromeda_postgresql_schema::csr::{Csr, RegistrationDeliveryState};
use andromeda_postgresql_seaquery::{
    clock::now,
    postgis::{repaired_when_invalid, st_geom_from_ewkb},
};
use andromeda_registration::{error::RegistrationError, store::forwarding_outcome::ForwardingOutcome};
use chrono::{DateTime, Utc};
use sea_query::{Expr as SeaExpr, ExprTrait, InsertStatement, Query, SimpleExpr, UpdateStatement};

/// The three columns one registration geometry is written to.
#[derive(Clone, Copy)]
pub(super) struct GeometryColumns {
    /// The geometry as given (clause 4.7.1).
    pub(super) written: Csr,
    /// Its `ST_MakeValid` form when it is invalid, which clause 4.7.2 admits
    /// because IETF RFC 7946 section 3.1.6 sets no simplicity rule on a ring.
    pub(super) repaired: Csr,
    /// The rectangle its great-circle edges cover, where it bows past the
    /// planar bounding box.
    pub(super) reach: Csr,
}

/// The writes of one registration geometry to its [`GeometryColumns`]:
/// `ST_GeomFromEWKB(ewkb)`, its repair, and `ST_GeomFromEWKB(reach)`. The EWKB
/// carries its SRID, and an absent EWKB writes NULL because every function is
/// strict.
pub(super) fn geometry_writes(columns: GeometryColumns, ewkb: Option<Vec<u8>>, reach: Option<Vec<u8>>) -> [(Csr, SimpleExpr); 3] {
    let geometry = st_geom_from_ewkb(SeaExpr::val(ewkb));
    // clone: the written column and its repair both read the geometry.
    [
        (columns.written, geometry.clone()),
        (columns.repaired, repaired_when_invalid(geometry)),
        (columns.reach, st_geom_from_ewkb(SeaExpr::val(reach))),
    ]
}

/// The registration insert of `columns`, the values in the order of the
/// columns beside them, stamped with the transaction's instant.
///
/// # Errors
///
/// Returns [`RegistrationError`] when the insert cannot be assembled.
pub(super) fn csr_create_statement(id: &Identifier, columns: Vec<(Csr, SimpleExpr)>) -> Result<InsertStatement, RegistrationError> {
    let (names, values): (Vec<Csr>, Vec<SimpleExpr>) = columns.into_iter().unzip();
    let mut insert = Query::insert();
    insert
        .into_table(Csr::Table)
        .columns([Csr::Id].into_iter().chain(names).chain([Csr::CreatedAt, Csr::ModifiedAt]))
        .values([SeaExpr::val(id.as_str())].into_iter().chain(values).chain([now(), now()]))
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(insert)
}

/// The registration replacement writing `columns` to the row `id` names.
pub(super) fn csr_update_statement(id: &Identifier, columns: Vec<(Csr, SimpleExpr)>) -> UpdateStatement {
    Query::update()
        .table(Csr::Table)
        .values(columns.into_iter().chain([(Csr::ModifiedAt, now())]))
        .and_where(SeaExpr::col(Csr::Id).eq(id.as_str()))
        .to_owned()
}

/// The delivery-state row a new registration starts with: nothing sent,
/// nothing failed, and an `ok` status the first forwarding outcome replaces.
///
/// # Errors
///
/// Returns [`RegistrationError`] when the fixed insert cannot be assembled.
pub(super) fn delivery_state_insert(id: &Identifier) -> Result<InsertStatement, RegistrationError> {
    let mut insert = Query::insert();
    insert
        .into_table(RegistrationDeliveryState::Table)
        .columns([RegistrationDeliveryState::RegistrationId, RegistrationDeliveryState::Status])
        .values([SeaExpr::val(id.as_str()), SeaExpr::val(stored_status(CsrStatus::Ok).code())])
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(insert)
}

/// Encode supported operations as their stored registry codes.
///
/// An empty list is stored as the absent member rather than as an empty array:
/// the column's check admits no empty set, the same rule the routing
/// projection's replicated copy follows.
pub(super) fn operation_codes(operations: &[CsrOperation]) -> Option<Vec<i16>> {
    (!operations.is_empty()).then(|| operations.iter().map(|operation| stored_operation(*operation).code()).collect())
}

/// Encode the stored forwarding mode.
pub(super) const fn mode_code(mode: ForwardingMode) -> i16 {
    stored_mode(mode).code()
}

/// A registration's clause 4.18 scopes in the two columns they are stored in:
/// `scope_paths`, the label paths a `scopeQ` matches, and `verbatim_scope_paths`, the text
/// read back. Both are absent together, and both hold the scopes in order.
pub(super) struct StoredScopes<'a> {
    /// The `ltree` label paths, bound to `scope_paths`.
    pub(super) label_paths: Option<Vec<String>>,
    /// The scopes as written, bound to `verbatim_scope_paths`.
    pub(super) written: Option<Vec<&'a str>>,
}

/// Encode a registration's scopes through the one stored form of a Scope.
pub(super) fn stored_scopes(scopes: Option<&[ScopePath]>) -> StoredScopes<'_> {
    let (label_paths, written) = scopes
        .map(|values| {
            values
                .iter()
                .map(|scope| {
                    let stored = StoredScopePath::from(scope);
                    (stored.label_path, stored.written)
                })
                .unzip()
        })
        .unzip();
    StoredScopes { label_paths, written }
}

/// Encode the `datasetId` list in its clause 4.5.5 wire form.
pub(super) fn dataset_selectors(dataset_id: Option<&[DatasetSelector]>) -> Option<Vec<String>> {
    dataset_id.map(|values| values.iter().map(ToString::to_string).collect())
}

/// A clause 5.2.11 interval as its two columns, `startAt` then `endAt`; both
/// `NULL` when the member is absent.
pub(super) fn interval_columns(interval: Option<&TimeInterval>) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    (interval.map(|interval| interval.start_at), interval.and_then(|interval| interval.end_at))
}

/// A registration's `management` (Table 5.2.34-1) as its columns.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ManagementColumns {
    /// Whether the member is present, empty or not.
    pub(super) declared: bool,
    /// `cacheDuration`, as the ISO 8601 text its type writes.
    pub(super) cache_duration: Option<String>,
    /// `cooldown`, in milliseconds.
    pub(super) cooldown: Option<i64>,
    /// `localOnly`.
    pub(super) local_only: Option<bool>,
    /// `timeout`, in milliseconds.
    pub(super) timeout: Option<i64>,
}

impl ManagementColumns {
    /// The columns of `management`, every member `NULL` when it is absent.
    pub(super) fn of(management: Option<&RegistrationManagementInfo>) -> ManagementColumns {
        ManagementColumns {
            declared: management.is_some(),
            cache_duration: management.and_then(|management| management.cache_duration.as_ref()).map(ToString::to_string),
            cooldown: management.and_then(|management| management.cooldown).map(i64::from),
            local_only: management.and_then(|management| management.local_only).map(bool::from),
            timeout: management.and_then(|management| management.timeout).map(i64::from),
        }
    }
}

/// A registration's `contextSourceInfo` as its two parallel columns, keys
/// then values, in written order; both `NULL` when the member is absent.
pub(super) fn context_source_info_columns(pairs: Option<&[KeyValuePair]>) -> (Option<Vec<&str>>, Option<Vec<&str>>) {
    pairs
        .map(|pairs| pairs.iter().map(|pair| (pair.key.as_str(), pair.value.as_str())).unzip())
        .unzip()
}

/// The health update one forwarding outcome applies (Table 5.2.9-2) to the
/// registration's delivery state: the sent counter always grows; the failed
/// counter, the matching timestamp and the status follow the outcome.
pub(super) fn health_update(id: &Identifier, outcome: ForwardingOutcome, at: DateTime<Utc>) -> UpdateStatement {
    let mut update = Query::update();
    update
        .table(RegistrationDeliveryState::Table)
        .value(RegistrationDeliveryState::TimesSent, SeaExpr::col(RegistrationDeliveryState::TimesSent).add(1))
        .and_where(SeaExpr::col(RegistrationDeliveryState::RegistrationId).eq(id.as_str()));
    match outcome {
        ForwardingOutcome::Succeeded => {
            update
                .value(RegistrationDeliveryState::LastSuccess, at)
                .value(RegistrationDeliveryState::Status, stored_status(CsrStatus::Ok).code());
        }
        ForwardingOutcome::Failed => {
            update
                .value(
                    RegistrationDeliveryState::TimesFailed,
                    SeaExpr::col(RegistrationDeliveryState::TimesFailed).add(1),
                )
                .value(RegistrationDeliveryState::LastFailure, at)
                .value(RegistrationDeliveryState::Status, stored_status(CsrStatus::Failed).code());
        }
    }
    update
}

/// The predicate matching proxied registrations: mode `exclusive` or `redirect`.
pub(super) fn proxied_mode() -> SimpleExpr {
    SeaExpr::col((Csr::Table, Csr::Mode)).is_in([mode_code(ForwardingMode::Exclusive), mode_code(ForwardingMode::Redirect)])
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::csource_registration::{
        registration_columns::{RegistrationFields, registration_columns},
        registration_statements::{
            ManagementColumns,
            context_source_info_columns,
            csr_create_statement,
            csr_update_statement,
            delivery_state_insert,
            health_update,
            interval_columns,
            proxied_mode,
            stored_scopes,
        },
    };
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, scope::path::ScopePath, time::date_time::NgsiLdDateTime};
    use andromeda_ngsi_ld_model::{key_value_pair::KeyValuePair, registration_management::RegistrationManagementInfo, time_interval::TimeInterval};
    use andromeda_postgresql_schema::csr::Csr;
    use andromeda_postgresql_seaquery::external_arguments::ExternalArguments;
    use andromeda_registration::store::{create_params::CreateCsrParams, forwarding_outcome::ForwardingOutcome};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};
    use serde_json::json;

    /// Clause 4.18 scopes are stored as `ltree` label paths, the encoding
    /// `entity_search.scope_paths` uses, so the clause 4.19 `scopeQ` compiles to
    /// one `lquery` predicate for both relations; the text beside them is what
    /// reads back, so each Scope keeps the spelling it was written in.
    #[test]
    fn registration_scopes_bind_as_label_paths_and_written_text_in_declared_order() {
        let scopes: Vec<ScopePath> = ["/Madrid/Gardens", "Madrid", "/a/b/c"]
            .into_iter()
            .map(|raw| raw.parse::<ScopePath>().expect("valid scope path"))
            .collect();
        let stored = stored_scopes(Some(&scopes));
        assert_eq!(
            stored.label_paths,
            Some(vec!["Madrid.Gardens".to_owned(), "Madrid".to_owned(), "a.b.c".to_owned()])
        );
        assert_eq!(stored.written, Some(vec!["/Madrid/Gardens", "Madrid", "/a/b/c"]));
        let absent = stored_scopes(None);
        assert_eq!((absent.label_paths, absent.written), (None, None), "an absent scope stays NULL in both columns");
    }

    fn registration() -> Identifier {
        "urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid identifier")
    }

    /// The columns a registration with nothing optional writes.
    fn columns() -> Vec<(Csr, SimpleExpr)> {
        let params = CreateCsrParams::builder()
            .information(Vec::new())
            .endpoint("https://example.org/csource".parse().expect("a dereferenceable URI"))
            .operations(Vec::new())
            .build();
        let mut external = ExternalArguments::default();
        registration_columns(&RegistrationFields::from(&params), None, &mut external).expect("the columns build")
    }

    #[test]
    fn the_create_writes_no_type_and_binds_the_mode_code_and_ltree_scopes() {
        let (sql, values) = csr_create_statement(&registration(), columns())
            .expect("the registration insert builds")
            .build(PostgresQueryBuilder);
        assert!(!sql.contains("ContextSourceRegistration"), "the type is never stored: {sql}");
        assert!(!sql.contains("\"type\""), "{sql}");
        assert!(!sql.contains("::csr_mode"), "{sql}");
        assert!(sql.contains("$27::ltree[]"), "{sql}");
        for marker in [10, 14, 18] {
            let (tested, repaired, reach) = (marker + 1, marker + 2, marker + 3);
            assert!(
                sql.contains(&format!(
                    "ST_GeomFromEWKB(${marker}), (CASE WHEN (ST_IsValid(ST_GeomFromEWKB(${tested}))) THEN NULL ELSE ST_MakeValid(ST_GeomFromEWKB(${repaired})) END), ST_GeomFromEWKB(${reach})"
                )),
                "geometry {marker} is written as given beside its repair and its reach: {sql}"
            );
        }
        assert!(sql.contains("NOW()"), "created_at and modified_at stamp the transaction clock: {sql}");
        assert!(sql.contains(r#""verbatim_scope_paths""#), "{sql}");
        assert_eq!(values.0.len(), 38, "every column but the Context Source Properties is a bound value: {sql}");
    }

    /// The rendering revision of the registration's pattern restatements is
    /// written in the statement that writes the registration, on create and on
    /// replacement alike; the restatements themselves are information rows.
    #[test]
    fn the_pattern_forms_revision_is_written_with_the_registration() {
        let (create, _) = csr_create_statement(&registration(), columns())
            .expect("the registration insert builds")
            .build(PostgresQueryBuilder);
        assert!(create.contains(r#""entity_id_pattern_forms_revision""#), "{create}");
        assert!(!create.contains(r#""information""#), "{create}");
        let (update, _) = csr_update_statement(&registration(), columns()).build(PostgresQueryBuilder);
        assert!(update.contains(r#""entity_id_pattern_forms_revision" = $36"#), "{update}");
        assert!(!update.contains(r#""entity_id_pattern_forms""#), "{update}");
    }

    #[test]
    fn the_update_pins_the_row_by_id_and_binds_the_mode_code_and_ltree_scopes() {
        let (sql, _) = csr_update_statement(&registration(), columns()).build(PostgresQueryBuilder);
        assert!(sql.contains(r#""id" = $38"#), "{sql}");
        assert!(sql.contains(r#""mode" = $24"#), "{sql}");
        assert!(sql.contains(r#""scope_paths" = $26::ltree[]"#), "{sql}");
        assert!(sql.contains(r#""verbatim_scope_paths" = $37"#), "{sql}");
        assert!(sql.contains(r#""modified_at" = NOW()"#), "{sql}");
        assert!(
            sql.contains(r#""location" = ST_GeomFromEWKB($9), "location_valid" = (CASE WHEN (ST_IsValid(ST_GeomFromEWKB($10))) THEN NULL ELSE ST_MakeValid(ST_GeomFromEWKB($11)) END), "location_reach" = ST_GeomFromEWKB($12)"#),
            "{sql}"
        );
    }

    #[test]
    fn a_new_registration_starts_its_delivery_state_as_ok() {
        let (sql, values) = delivery_state_insert(&registration())
            .expect("the fixed delivery-state insert builds")
            .build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"INSERT INTO "registration_delivery_state" ("registration_id", "status") VALUES ($1, $2)"#
        );
        assert_eq!(values.0.len(), 2);
    }

    #[test]
    fn a_forwarding_outcome_updates_the_delivery_state_not_the_registration() {
        let at = instant("2026-01-01T00:00:00Z");
        let (success, _) = health_update(&registration(), ForwardingOutcome::Succeeded, at).build(PostgresQueryBuilder);
        assert!(success.starts_with(r#"UPDATE "registration_delivery_state""#), "{success}");
        assert!(success.contains(r#""times_sent" = "times_sent" + $1"#), "{success}");
        assert!(success.contains(r#""last_success" = $2"#), "{success}");
        assert!(success.contains(r#""status" = $3"#), "{success}");
        assert!(!success.contains("times_failed"), "{success}");
        let (failure, _) = health_update(&registration(), ForwardingOutcome::Failed, at).build(PostgresQueryBuilder);
        assert!(failure.contains(r#""times_failed" = "times_failed" + $2"#), "{failure}");
        assert!(failure.contains(r#""last_failure" = $3"#), "{failure}");
        assert!(failure.contains(r#""status" = $4"#), "{failure}");
        assert!(failure.contains(r#""registration_id" = $5"#), "{failure}");
    }

    #[test]
    fn operations_are_stored_as_codes_and_an_empty_list_as_the_absent_member() {
        use crate::csource_registration::registration_statements::operation_codes;
        use andromeda_ngsi_ld_model::registration::operation::CsrOperation;
        assert_eq!(operation_codes(&[CsrOperation::CreateEntity, CsrOperation::RedirectionOps]), Some(vec![1, 48]));
        assert_eq!(operation_codes(&[]), None);
    }

    #[test]
    fn a_proxied_registration_is_exclusive_or_redirect_by_code() {
        let sql = Query::select().expr(proxied_mode()).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "context_source_registrations"."mode" IN (2, 3)"#);
    }

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    #[test]
    fn the_intervals_bind_as_four_instants() {
        let observation: TimeInterval = serde_json::from_value(json!({"startAt": "2026-01-01T00:00:00Z"})).expect("a valid interval");
        let management: TimeInterval =
            serde_json::from_value(json!({"startAt": "2026-01-01T00:00:00Z", "endAt": "2026-02-01T00:00:00Z"})).expect("a valid interval");
        assert_eq!(interval_columns(Some(&observation)), (Some(instant("2026-01-01T00:00:00Z")), None));
        assert_eq!(
            interval_columns(Some(&management)),
            (Some(instant("2026-01-01T00:00:00Z")), Some(instant("2026-02-01T00:00:00Z")))
        );
        assert_eq!(interval_columns(None), (None, None));
    }

    #[test]
    fn the_management_members_bind_as_four_columns() {
        let management: RegistrationManagementInfo =
            serde_json::from_value(json!({"cacheDuration": "PT10S", "cooldown": 500, "localOnly": true, "timeout": 3000})).expect("valid management");
        assert_eq!(
            ManagementColumns::of(Some(&management)),
            ManagementColumns {
                declared: true,
                cache_duration: Some("PT10S".to_owned()),
                cooldown: Some(500),
                local_only: Some(true),
                timeout: Some(3000),
            }
        );
    }

    /// An empty `management` object is declared with every member `NULL`, so
    /// it reads back as the empty object it was written as.
    #[test]
    fn an_absent_management_binds_nulls() {
        let absent = ManagementColumns {
            declared: false,
            cache_duration: None,
            cooldown: None,
            local_only: None,
            timeout: None,
        };
        assert_eq!(ManagementColumns::of(None), absent);
        let empty: RegistrationManagementInfo = serde_json::from_value(json!({})).expect("valid management");
        assert_eq!(ManagementColumns::of(Some(&empty)), ManagementColumns { declared: true, ..absent });
    }

    #[test]
    fn the_context_source_info_binds_as_two_parallel_arrays() {
        let pairs: Vec<KeyValuePair> = serde_json::from_value(json!([{"key": "a", "value": "1"}, {"key": "b", "value": "2"}])).expect("valid pairs");
        assert_eq!(context_source_info_columns(Some(&pairs)), (Some(vec!["a", "b"]), Some(vec!["1", "2"])));
        assert_eq!(context_source_info_columns(Some(&[])), (Some(Vec::new()), Some(Vec::new())));
        assert_eq!(context_source_info_columns(None), (None, None));
    }
}
