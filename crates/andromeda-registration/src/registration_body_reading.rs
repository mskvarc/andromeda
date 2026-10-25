//! Reading a [`RegistrationBody`] member by member from the JSON it arrived as.
//!
//! The members of Table 5.2.9-1 are read into the types that own their
//! restrictions, the read-only members of Table 5.2.9-2 are read and set aside,
//! and every other member becomes a Context Source Property held as data
//! (clause 4.9). A Context Source Property is captured as JSON of its own, so
//! the JSON `null` the body reader refuses everywhere else is refused here. The reading is written by hand rather than derived because the
//! catch-all a derive offers (`#[serde(flatten)]`) buffers the whole body,
//! geometries included, before reading any of it.

use crate::{csr_member::CsrMember, registration_body::RegistrationBody};
use andromeda_json_literal::{JsonLiteral, piece::JsonKind};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{clearable::Clearable, clearable_array};
use andromeda_ngsi_ld_vocabulary::keyword;
use serde::{
    Deserialize,
    Deserializer,
    de::{Error as DeserializeError, IgnoredAny, MapAccess, Unexpected, Visitor},
};
use std::{fmt, str::FromStr};
use thiserror::Error;

/// A Context Source Property a body names more than once.
#[derive(Debug, Error)]
#[error("the Context Source Property {0} is named twice")]
struct DuplicateProperty(AttributeName);

/// The read-only members of Table 5.2.9-2, which a body may repeat from a
/// retrieved registration and which no client sets.
const READ_ONLY_MEMBERS: [&str; 7] = ["status", "timesSent", "timesFailed", "lastSuccess", "lastFailure", "createdAt", "modifiedAt"];

/// What a member name of the body names.
enum MemberName {
    /// `id`, the identifier the client gives the registration.
    Id,
    /// A member read and set aside.
    Ignored,
    /// A member of Table 5.2.9-1.
    Defined(CsrMember),
    /// A Context Source Property.
    Property(AttributeName),
}

impl<'de> Deserialize<'de> for MemberName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<MemberName, D::Error> {
        deserializer.deserialize_str(MemberNameVisitor)
    }
}

/// Reads one member name.
struct MemberNameVisitor;

impl Visitor<'_> for MemberNameVisitor {
    type Value = MemberName;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a member name")
    }

    fn visit_str<E: DeserializeError>(self, name: &str) -> Result<MemberName, E> {
        if name == "id" {
            return Ok(MemberName::Id);
        }
        if name == keyword::CONTEXT || READ_ONLY_MEMBERS.contains(&name) {
            return Ok(MemberName::Ignored);
        }
        match CsrMember::from_str(name) {
            Ok(member) => Ok(MemberName::Defined(member)),
            Err(_not_defined) => AttributeName::from_str(name).map(MemberName::Property).map_err(E::custom),
        }
    }
}

impl<'de> Deserialize<'de> for RegistrationBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<RegistrationBody, D::Error> {
        deserializer.deserialize_map(RegistrationBodyVisitor)
    }
}

/// Reads the members of one body.
struct RegistrationBodyVisitor;

impl<'de> Visitor<'de> for RegistrationBodyVisitor {
    type Value = RegistrationBody;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a ContextSourceRegistration object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<RegistrationBody, A::Error> {
        let mut body = RegistrationBody::default();
        while let Some(name) = members.next_key::<MemberName>()? {
            match name {
                MemberName::Id => {
                    if body.id.is_some() {
                        return Err(A::Error::duplicate_field("id"));
                    }
                    body.id = Some(members.next_value::<Identifier>()?);
                }
                MemberName::Ignored => {
                    members.next_value::<IgnoredAny>()?;
                }
                MemberName::Defined(member) => read_defined(&mut body, member, &mut members)?,
                MemberName::Property(name) => {
                    let value = members.next_value::<JsonLiteral>()?;
                    // Clause 5.2.1: a property admits no null value directly.
                    if value.piece().kind() == JsonKind::Null {
                        return Err(A::Error::invalid_type(Unexpected::Unit, &"a Context Source Property"));
                    }
                    if body.additional_properties.contains_key(&name) {
                        return Err(A::Error::custom(DuplicateProperty(name)));
                    }
                    body.additional_properties.insert(name, value);
                }
            }
        }
        Ok(body)
    }
}

/// Read the value of the member `member` into the slot of `body` that holds it.
fn read_defined<'de, A: MapAccess<'de>>(body: &mut RegistrationBody, member: CsrMember, members: &mut A) -> Result<(), A::Error> {
    match member {
        CsrMember::Type => read(&mut body.type_, member, members),
        CsrMember::Information => read_array(&mut body.information, member, members),
        CsrMember::Mode => read(&mut body.mode, member, members),
        CsrMember::Operations => read_array(&mut body.operations, member, members),
        CsrMember::RegistrationName => read(&mut body.registration_name, member, members),
        CsrMember::Description => read(&mut body.description, member, members),
        CsrMember::DatasetId => read_array(&mut body.dataset_id, member, members),
        CsrMember::Tenant => read(&mut body.tenant, member, members),
        CsrMember::ObservationInterval => read(&mut body.observation_interval, member, members),
        CsrMember::ManagementInterval => read(&mut body.management_interval, member, members),
        CsrMember::Location => read(&mut body.location, member, members),
        CsrMember::ObservationSpace => read(&mut body.observation_space, member, members),
        CsrMember::OperationSpace => read(&mut body.operation_space, member, members),
        CsrMember::ExpiresAt => read(&mut body.expires_at, member, members),
        CsrMember::Endpoint => read(&mut body.endpoint, member, members),
        CsrMember::Accept => read(&mut body.accept, member, members),
        CsrMember::Scope => read_array(&mut body.scope, member, members),
        CsrMember::RefreshRate => read(&mut body.refresh_rate, member, members),
        CsrMember::Management => read(&mut body.management, member, members),
        CsrMember::ContextSourceInfo => read_array(&mut body.context_source_info, member, members),
        CsrMember::ContextSourceAlias => read(&mut body.context_source_alias, member, members),
    }
}

/// Read the value of `member` into `slot`, which a member written twice would
/// find taken.
fn read<'de, T: Deserialize<'de>, A: MapAccess<'de>>(slot: &mut Clearable<T>, member: CsrMember, members: &mut A) -> Result<(), A::Error> {
    if !slot.is_absent() {
        return Err(A::Error::duplicate_field(member.key()));
    }
    *slot = members.next_value()?;
    Ok(())
}

/// Read the value of `member`, whose data type is an array, into `slot`: an
/// array holding the NGSI-LD Null alone deletes it as the bare Null does
/// (clause 4.5.0).
fn read_array<'de, E: Deserialize<'de>, A: MapAccess<'de>>(slot: &mut Clearable<Vec<E>>, member: CsrMember, members: &mut A) -> Result<(), A::Error> {
    if !slot.is_absent() {
        return Err(A::Error::duplicate_field(member.key()));
    }
    *slot = members.next_value::<ArrayMember<E>>()?.0;
    Ok(())
}

/// An array member, read by [`clearable_array`].
#[derive(Deserialize)]
#[serde(transparent, bound = "E: Deserialize<'de>")]
struct ArrayMember<E>(#[serde(deserialize_with = "clearable_array::deserialize")] Clearable<Vec<E>>);

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::registration_body::RegistrationBody;
    use andromeda_ngsi_ld_model::clearable::Clearable;

    #[test]
    fn a_member_written_twice_is_refused() {
        let error = serde_json::from_str::<RegistrationBody>(r#"{"description": "a", "description": "b"}"#).expect_err("a duplicate member");
        assert!(error.to_string().contains("duplicate field `description`"), "{error}");
        assert!(serde_json::from_str::<RegistrationBody>(r#"{"custom": 1, "custom": 2}"#).is_err());
    }

    #[test]
    fn a_member_that_is_no_name_is_refused() {
        assert!(serde_json::from_str::<RegistrationBody>(r#"{"not a name": 1}"#).is_err());
    }

    #[test]
    fn the_read_only_members_of_a_retrieved_registration_are_set_aside() {
        let body = serde_json::from_str::<RegistrationBody>(
            r#"{"status": "ok", "timesSent": 3, "createdAt": "2024-01-01T00:00:00Z", "modifiedAt": "2024-01-02T00:00:00Z", "mode": "redirect"}"#,
        )
        .expect("a retrieved registration is a body");
        assert!(body.additional_properties.is_empty());
        assert!(matches!(body.mode, Clearable::Set(_)));
    }

    /// Clause 5.2.1: a Context Source Property admits no JSON `null`, while a
    /// `null` it holds as data is kept.
    #[test]
    fn a_context_source_property_that_is_a_json_null_is_refused() {
        assert!(serde_json::from_str::<RegistrationBody>(r#"{"custom": null}"#).is_err());
        let body = serde_json::from_str::<RegistrationBody>(r#"{"custom": {"a": null}}"#).expect("data holding a null");
        pretty_assertions::assert_eq!(body.additional_properties.len(), 1);
    }

    /// An array member written twice is refused as any member is.
    #[test]
    fn an_array_member_written_twice_is_refused() {
        assert!(serde_json::from_str::<RegistrationBody>(r#"{"scope": ["/a"], "scope": "urn:ngsi-ld:null"}"#).is_err());
    }

    #[test]
    fn a_body_that_is_no_object_is_refused() {
        assert!(serde_json::from_str::<RegistrationBody>("[]").is_err());
        assert!(serde_json::from_str::<RegistrationBody>(r#""registration""#).is_err());
    }
}
