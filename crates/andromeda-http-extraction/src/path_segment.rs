//! Route parameters named by the specification parameter they carry.
//!
//! A route's own keys are labels for routing and metrics. What a refusal names
//! is the parameter of the clause 6 resource template the segment fills
//! (`entities/{entityId}/attrs/{attrId}`), so each segment type carries that
//! name and parses its value through the domain type's own grammar.

use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    identifier::{Identifier, IdentifierParseError},
    reference::NgsiLdReferenceError,
};
use strum::Display;
use thiserror::Error;

/// A path parameter of a clause 6 resource template, spelled as the
/// specification spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "camelCase")]
pub enum PathParameter {
    /// `entities/{entityId}` and `temporal/entities/{entityId}`.
    EntityId,
    /// `.../attrs/{attrId}` and `attributes/{attrId}`.
    AttrId,
    /// `temporal/entities/{entityId}/attrs/{attrId}/{instanceId}`.
    InstanceId,
    /// `types/{type}`.
    Type,
    /// `csourceRegistrations/{registrationId}`.
    RegistrationId,
    /// `subscriptions/{subscriptionId}` and `csourceSubscriptions/{subscriptionId}`.
    SubscriptionId,
    /// `snapshots/{snapshotId}`.
    SnapshotId,
    /// `entityMaps/{entityMapId}`.
    EntityMapId,
    /// `jsonldContexts/{contextId}`.
    ContextId,
}

/// Why a segment's value is not a value of its domain type.
#[derive(Debug, Error)]
pub enum SegmentFault {
    /// The segment is not an identifier.
    #[error(transparent)]
    Identifier(#[from] IdentifierParseError),
    /// The segment is not a name.
    #[error(transparent)]
    Name(#[from] NgsiLdReferenceError),
}

/// A path segment whose value does not parse as its parameter's type.
#[derive(Debug, Error)]
#[error("invalid path parameter {parameter}: {fault}")]
pub struct InvalidPathSegment {
    /// The parameter the segment fills.
    pub parameter: PathParameter,
    /// Why the value does not parse.
    #[source]
    pub fault: SegmentFault,
}

/// One route parameter, typed and named after its specification parameter.
pub trait PathSegment: Sized {
    /// The specification parameter the segment fills.
    const PARAMETER: PathParameter;

    /// The segment read from its percent-decoded text.
    ///
    /// # Errors
    ///
    /// Returns the [`SegmentFault`] of a value its domain type refuses.
    fn parse(segment: &str) -> Result<Self, SegmentFault>;

    /// The segment read from `segment`, its refusal naming the parameter.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathSegment`] for a value its domain type refuses.
    fn read(segment: &str) -> Result<Self, InvalidPathSegment> {
        Self::parse(segment).map_err(|fault| InvalidPathSegment {
            parameter: Self::PARAMETER,
            fault,
        })
    }
}

/// Declare a path segment newtype over a domain type parsed by its `FromStr`.
macro_rules! path_segment {
    ($(#[$meta:meta])* $name:ident($value:ty) = $parameter:ident) => {
        $(#[$meta])*
        #[derive(Debug)]
        pub struct $name(pub $value);

        impl PathSegment for $name {
            const PARAMETER: PathParameter = PathParameter::$parameter;

            fn parse(segment: &str) -> Result<$name, SegmentFault> {
                segment.parse().map($name).map_err(SegmentFault::from)
            }
        }
    };
}

path_segment! {
    /// The entity an `entities/{entityId}` resource addresses.
    EntityId(Identifier) = EntityId
}

path_segment! {
    /// The attribute an `attrs/{attrId}` or `attributes/{attrId}` resource addresses.
    AttrId(AttributeName) = AttrId
}

path_segment! {
    /// The attribute instance a temporal `{instanceId}` resource addresses.
    InstanceId(Identifier) = InstanceId
}

path_segment! {
    /// The entity type a `types/{type}` resource addresses.
    EntityType(EntityTypeName) = Type
}

path_segment! {
    /// The registration a `csourceRegistrations/{registrationId}` resource addresses.
    RegistrationId(Identifier) = RegistrationId
}

path_segment! {
    /// The subscription a `subscriptions/{subscriptionId}` or
    /// `csourceSubscriptions/{subscriptionId}` resource addresses.
    SubscriptionId(Identifier) = SubscriptionId
}

path_segment! {
    /// The Snapshot a `snapshots/{snapshotId}` resource addresses.
    SnapshotId(Identifier) = SnapshotId
}

path_segment! {
    /// The Entity Map an `entityMaps/{entityMapId}` resource addresses.
    EntityMapId(Identifier) = EntityMapId
}

/// The route parameters of one route: a single segment, or a tuple of them in
/// route order.
pub trait PathSegments: Sized {
    /// The percent-decoded text of the segments, as the router hands it over.
    type Raw;

    /// The segments read from their text.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathSegment`] for the first segment its domain type
    /// refuses.
    fn read_segments(raw: Self::Raw) -> Result<Self, InvalidPathSegment>;
}

impl<A: PathSegment> PathSegments for A {
    type Raw = String;

    fn read_segments(raw: String) -> Result<A, InvalidPathSegment> {
        A::read(&raw)
    }
}

impl<A: PathSegment, B: PathSegment> PathSegments for (A, B) {
    type Raw = (String, String);

    fn read_segments((a, b): (String, String)) -> Result<(A, B), InvalidPathSegment> {
        Ok((A::read(&a)?, B::read(&b)?))
    }
}

impl<A: PathSegment, B: PathSegment, C: PathSegment> PathSegments for (A, B, C) {
    type Raw = (String, String, String);

    fn read_segments((a, b, c): (String, String, String)) -> Result<(A, B, C), InvalidPathSegment> {
        Ok((A::read(&a)?, B::read(&b)?, C::read(&c)?))
    }
}

#[cfg(test)]
mod tests {
    use crate::path_segment::{AttrId, EntityId, EntityType, InstanceId, PathParameter, PathSegment, PathSegments};

    #[test]
    fn a_refused_segment_names_its_specification_parameter() {
        for (refusal, parameter) in [
            (EntityId::read("bare-name").expect_err("no URI").parameter, PathParameter::EntityId),
            (AttrId::read(" ").expect_err("no name").parameter, PathParameter::AttrId),
            (InstanceId::read("bare").expect_err("no URI").parameter, PathParameter::InstanceId),
            (EntityType::read("").expect_err("empty").parameter, PathParameter::Type),
        ] {
            pretty_assertions::assert_eq!(refusal, parameter);
        }
    }

    #[test]
    fn the_detail_names_the_parameter_as_the_specification_spells_it() {
        let detail = EntityId::read("bare-name").expect_err("no URI").to_string();
        assert!(detail.starts_with("invalid path parameter entityId: "), "{detail}");
        pretty_assertions::assert_eq!(PathParameter::Type.to_string(), "type");
        pretty_assertions::assert_eq!(PathParameter::EntityMapId.to_string(), "entityMapId");
    }

    #[test]
    fn a_tuple_names_the_segment_at_fault() {
        let refusal =
            <(EntityId, AttrId, InstanceId) as PathSegments>::read_segments(("urn:ngsi-ld:Vehicle:1".to_owned(), "speed".to_owned(), "bare".to_owned()))
                .expect_err("the instance is no URI");
        pretty_assertions::assert_eq!(refusal.parameter, PathParameter::InstanceId);
    }
}
