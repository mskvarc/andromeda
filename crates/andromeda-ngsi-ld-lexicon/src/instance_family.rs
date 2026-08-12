//! The family a temporal instance identifier is minted under.

use crate::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use serde::{Deserialize, Serialize};

/// Which series of temporal instances an identifier names.
///
/// Attribute history and scope history are separate series, and clause 4.5.7
/// requires an `instanceId` to identify one instance, so each series mints its
/// broker-generated identifiers under its own URN template. Both templates are
/// permanently stable: an identifier already handed to a client has to keep
/// rendering byte for byte for the lifetime of the deployment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstanceFamily {
    /// Instances of one attribute's temporal history.
    Attribute,
    /// Instances of one entity's scope history.
    Scope,
}

impl InstanceFamily {
    /// The namespace-specific prefix a generated identifier of this family carries.
    pub const fn nss_prefix(self) -> &'static str {
        match self {
            InstanceFamily::Attribute => "attribute:instance:",
            InstanceFamily::Scope => "scope:instance:",
        }
    }

    /// The family the history of `attribute_name` belongs to.
    ///
    /// Clause 4.18 gives `scope` an attribute-shaped temporal representation,
    /// so scope history reaches rendering keyed by the scope IRI rather than
    /// through a carrier of its own; that IRI is what separates the two
    /// families once a row is decoded.
    pub fn of_attribute_name(attribute_name: &AttributeName) -> InstanceFamily {
        if attribute_name.as_str() == expanded_properties::SCOPE.as_str() {
            InstanceFamily::Scope
        } else {
            InstanceFamily::Attribute
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_name::AttributeName, instance_family::InstanceFamily};
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;

    #[test]
    fn the_scope_iri_names_the_scope_family_and_every_other_attribute_the_attribute_family() {
        let scope: AttributeName = expanded_properties::SCOPE.as_str().parse().expect("valid attribute name");
        let speed: AttributeName = "https://example.org/speed".parse().expect("valid attribute name");

        pretty_assertions::assert_eq!(InstanceFamily::of_attribute_name(&scope), InstanceFamily::Scope);
        pretty_assertions::assert_eq!(InstanceFamily::of_attribute_name(&speed), InstanceFamily::Attribute);
    }

    #[test]
    fn the_two_families_mint_under_different_prefixes() {
        pretty_assertions::assert_eq!(InstanceFamily::Attribute.nss_prefix(), "attribute:instance:");
        pretty_assertions::assert_eq!(InstanceFamily::Scope.nss_prefix(), "scope:instance:");
    }
}
