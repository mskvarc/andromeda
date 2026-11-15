//! The Context Source Registrations v1.0.0 stored, and the Context Source
//! Property document this build writes for each.

use crate::fixtures::{
    statements::{document, insert, smallint},
    test_error::UpgradeTestError,
};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use sea_query::Expr;
use serde_json::Value;
use sqlx::PgPool;

/// The members of a registration whose document v1.0.0 wrote otherwise than
/// this build: each time interval, under its core name and its expanded name.
const TIME_INTERVALS: [(&str, &str); 2] = [
    ("managementInterval", "https://uri.etsi.org/ngsi-ld/managementInterval"),
    ("observationInterval", "https://uri.etsi.org/ngsi-ld/observationInterval"),
];

/// The Context Source Property document this build writes for
/// `registration`, a registration in the form a client writes it.
pub(crate) fn written_document(registration: &Value) -> Result<Value, UpgradeTestError> {
    let registration: ContextSourceRegistration = serde_json::from_value(registration.clone())?;
    let document = registration.property_members().document()?;
    Ok(serde_json::from_str(document.into_literal().text())?)
}

/// The Context Source Property document v1.0.0 stored for `registration`: the
/// one this build writes, with each time interval as the registration itself
/// writes it, its instants plain strings.
pub(crate) fn stored_by_v1_0_0(registration: &Value) -> Result<Value, UpgradeTestError> {
    let model: ContextSourceRegistration = serde_json::from_value(registration.clone())?;
    let written = serde_json::to_value(&model)?;
    let mut document = written_document(registration)?;
    for (name, iri) in TIME_INTERVALS {
        if let Some(interval) = written.get(name) {
            document[iri] = interval.clone();
        }
    }
    Ok(document)
}

/// Inserts the row of a registration `id` as v1.0.0 wrote it, holding
/// `properties` as its Context Source Property document; the other columns
/// hold what the document needs no column for.
pub(crate) async fn insert_registration(pool: &PgPool, id: &str, properties: &Value) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "context_source_registrations",
        vec![
            ("id", Expr::val(id)),
            ("endpoint", Expr::val("https://source.example.com/")),
            ("mode", smallint(1)),
            ("is_management_declared", Expr::val(false)),
            ("context_source_properties", document(properties)),
        ],
    )
    .await
}
