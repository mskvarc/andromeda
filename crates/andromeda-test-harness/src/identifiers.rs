//! Structured globally unique identifiers for isolated test resources.

use iri_rs::IriBuf;
use thiserror::Error;
use uuid::Uuid;

/// Validated integration-test suite name used inside generated URNs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestSuiteName(String);

impl TestSuiteName {
    /// Validate a non-empty suite token containing ASCII letters, digits, `-`, or `_`.
    pub fn parse(value: impl Into<String>) -> Result<TestSuiteName, IdentifierError> {
        let value = value.into();
        if !value.is_empty()
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        {
            Ok(TestSuiteName(value))
        } else {
            Err(IdentifierError::InvalidSuiteName)
        }
    }

    /// Borrow the validated suite token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated local resource label appended to a generated namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceLabel(String);

impl ResourceLabel {
    /// Validate a non-empty URN path token.
    pub fn parse(value: impl Into<String>) -> Result<ResourceLabel, IdentifierError> {
        let value = value.into();
        if !value.is_empty()
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
        {
            Ok(ResourceLabel(value))
        } else {
            Err(IdentifierError::InvalidResourceLabel)
        }
    }
}

/// Namespace of a generated test resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    /// NGSI-LD Entity.
    Entity,
    /// Subscription.
    Subscription,
    /// Context Source Registration.
    Registration,
    /// Snapshot.
    Snapshot,
    /// JSON-LD context resource.
    JsonLdContext,
    /// Entity Map.
    EntityMap,
    /// Other explicitly scoped resource.
    Generic,
}

impl ResourceKind {
    const fn token(self) -> &'static str {
        match self {
            ResourceKind::Entity => "entity",
            ResourceKind::Subscription => "subscription",
            ResourceKind::Registration => "registration",
            ResourceKind::Snapshot => "snapshot",
            ResourceKind::JsonLdContext => "jsonld-context",
            ResourceKind::EntityMap => "entity-map",
            ResourceKind::Generic => "resource",
        }
    }
}

/// Identifier allocator belonging to one isolated test execution.
#[derive(Clone, Debug)]
pub struct TestIdentifiers {
    prefix: IriBuf,
}

impl TestIdentifiers {
    /// Allocate a process-independent unique test scope.
    pub fn allocate(suite: &TestSuiteName) -> Result<TestIdentifiers, IdentifierError> {
        let prefix = format!("urn:ngsi-ld:test:{}:{}:{}", suite.as_str(), std::process::id(), Uuid::new_v4());
        let prefix = prefix.parse().map_err(|_error| IdentifierError::GeneratedIri)?;
        Ok(TestIdentifiers { prefix })
    }

    /// Borrow the non-empty cleanup prefix shared by every allocated resource.
    pub const fn prefix(&self) -> &IriBuf {
        &self.prefix
    }

    /// Allocate one resource-specific identifier.
    pub fn resource(&self, kind: ResourceKind, label: &ResourceLabel) -> Result<IriBuf, IdentifierError> {
        format!("{}:{}:{}", self.prefix, kind.token(), label.0)
            .parse()
            .map_err(|_error| IdentifierError::GeneratedIri)
    }
}

/// Invalid suite, label, or generated test IRI.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum IdentifierError {
    /// The suite token is empty or contains unsupported characters.
    #[error("invalid test suite name")]
    InvalidSuiteName,
    /// The local resource label is empty or contains unsupported characters.
    #[error("invalid test resource label")]
    InvalidResourceLabel,
    /// An internal generated identifier failed IRI validation.
    #[error("generated test identifier is not a valid IRI")]
    GeneratedIri,
}

#[cfg(test)]
mod tests {
    use crate::identifiers::{ResourceKind, ResourceLabel, TestIdentifiers, TestSuiteName};

    #[test]
    fn allocators_are_unique_and_resource_namespaces_are_explicit() {
        let suite = TestSuiteName::parse("entity").unwrap_or_else(|error| unreachable!("static suite name is valid: {error}"));
        let label = ResourceLabel::parse("Building").unwrap_or_else(|error| unreachable!("static resource label is valid: {error}"));
        let first = TestIdentifiers::allocate(&suite).unwrap_or_else(|error| unreachable!("generated identifier is valid: {error}"));
        let second = TestIdentifiers::allocate(&suite).unwrap_or_else(|error| unreachable!("generated identifier is valid: {error}"));

        assert_ne!(first.prefix(), second.prefix());
        assert!(
            first
                .resource(ResourceKind::Entity, &label)
                .unwrap_or_else(|error| unreachable!("generated identifier is valid: {error}"))
                .as_str()
                .contains(":entity:")
        );
        assert!(
            first
                .resource(ResourceKind::Subscription, &label)
                .unwrap_or_else(|error| unreachable!("generated identifier is valid: {error}"))
                .as_str()
                .contains(":subscription:")
        );
    }
}
