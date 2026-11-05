//! The UUID a notification's `id` carries.

use iri_rs::{Iri, iri};
use std::fmt::Display;
use uuid::{Uuid, Version};

/// The kind prefix of a notification `id`, which a broker follows with the local id.
const NOTIFICATION_PREFIX: Iri<&str> = iri!("urn:ngsi-ld:Notification:");

/// The RFC 9562 version of the UUID after the `urn:ngsi-ld:Notification:` prefix of `id`, `None` when `id` is not a
/// notification identifier that ends in a UUID.
#[must_use]
pub fn uuid_version(id: &impl Display) -> Option<Version> {
    let text = id.to_string();
    let local_id = text.strip_prefix(NOTIFICATION_PREFIX.as_str())?;
    Uuid::parse_str(local_id).ok()?.get_version()
}

#[cfg(test)]
mod tests {
    use crate::notification_uuid::uuid_version;
    use uuid::Version;

    #[test]
    fn a_notification_identifier_reports_the_version_of_its_uuid() {
        assert_eq!(
            uuid_version(&"urn:ngsi-ld:Notification:7a2f0c3e-5f6d-5a8e-9c11-2b6d4e8f1a03"),
            Some(Version::Sha1)
        );
        assert_eq!(
            uuid_version(&"urn:ngsi-ld:Notification:7a2f0c3e-5f6d-4a8e-9c11-2b6d4e8f1a03"),
            Some(Version::Random)
        );
    }

    #[test]
    fn an_identifier_of_another_kind_or_without_a_uuid_has_no_version() {
        assert_eq!(uuid_version(&"urn:ngsi-ld:Subscription:7a2f0c3e-5f6d-5a8e-9c11-2b6d4e8f1a03"), None);
        assert_eq!(uuid_version(&"urn:ngsi-ld:Notification:n1"), None);
    }
}
