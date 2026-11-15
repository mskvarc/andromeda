//! The ids of the registrations a discovery answer or a context source notification carries, read without the rest of
//! each registration.
//!
//! A registrant's Context Source Property is kept as written (clause 5.2.9), so a broker serves one the typed
//! registration model refuses, such as a value object declaring the `Date` `2024-1-1`. A check that asks only which
//! registrations an answer holds reads their ids alone.

use ngsi_ld::{model::UriError, registration::RegistrationId};
use serde::Deserialize;
use std::collections::HashSet;

/// Why an answer's registration ids could not be read.
#[derive(Debug, thiserror::Error)]
pub enum RegistrationIdsError {
    /// The body is not the JSON shape it should be.
    #[error("the body is not a list of registrations")]
    Shape(#[source] serde_json::Error),
    /// A registration's `id` is not a URI.
    #[error("a registration id is not a URI")]
    Id(#[source] UriError),
}

/// One registration of an answer, of which only its `id` is read.
#[derive(Debug, Deserialize)]
struct Identified {
    /// The `id`, as the broker spells it.
    id: String,
}

/// A context source notification (clause 5.3.2), of which only the ids of its registrations are read.
#[derive(Debug, Deserialize)]
struct Notified {
    /// The registrations the notification reports.
    data: Vec<Identified>,
}

/// The ids of the registrations a Query Context Source Registrations answer body lists.
///
/// # Errors
///
/// Returns [`RegistrationIdsError`] when the body is no JSON array of objects carrying a URI `id`.
pub fn listed(body: &[u8]) -> Result<HashSet<RegistrationId>, RegistrationIdsError> {
    ids(serde_json::from_slice::<Vec<Identified>>(body).map_err(RegistrationIdsError::Shape)?)
}

/// The ids of the registrations a context source notification body reports.
///
/// # Errors
///
/// Returns [`RegistrationIdsError`] when the body is no notification whose `data` holds objects carrying a URI `id`.
pub fn notified(body: &[u8]) -> Result<HashSet<RegistrationId>, RegistrationIdsError> {
    ids(serde_json::from_slice::<Notified>(body).map_err(RegistrationIdsError::Shape)?.data)
}

/// The ids `registrations` carry, each read as a URI.
fn ids(registrations: Vec<Identified>) -> Result<HashSet<RegistrationId>, RegistrationIdsError> {
    registrations
        .into_iter()
        .map(|registration| registration.id.parse::<RegistrationId>().map_err(RegistrationIdsError::Id))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::registration_ids::{listed, notified};
    use ngsi_ld::registration::RegistrationId;
    use std::collections::HashSet;

    fn id(text: &str) -> RegistrationId {
        text.parse().expect("a registration id")
    }

    /// A Context Source Property the typed registration model refuses does not keep an answer's ids from being read.
    #[test]
    fn a_listing_is_read_by_its_ids_whatever_its_properties_hold() {
        let body = br#"[{"id": "urn:x:r1", "type": "ContextSourceRegistration", "day": {"@type": "Date", "@value": "2024-1-1"}}, {"id": "urn:x:r2"}]"#;
        assert_eq!(listed(body).expect("ids"), HashSet::from([id("urn:x:r1"), id("urn:x:r2")]));
    }

    #[test]
    fn a_notification_is_read_by_the_ids_of_its_data() {
        let body = br#"{"id": "urn:x:n", "type": "Notification", "data": [{"id": "urn:x:r1", "day": {"@type": "Date", "@value": "2024-02-30"}}]}"#;
        assert_eq!(notified(body).expect("ids"), HashSet::from([id("urn:x:r1")]));
    }

    #[test]
    fn an_id_that_is_no_uri_is_refused() {
        assert!(listed(br#"[{"id": "not a uri"}]"#).is_err());
    }
}
