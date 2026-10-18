//! What identifies one instance of a Temporal Evolution.

use andromeda_ngsi_ld_lexicon::instance_identity::InstanceIdentity;

/// The identity one decoded temporal instance carries.
///
/// Clause 4.5.7 says a system should maintain an `instanceId` for every
/// instance of a Property, and clause 4.5.8 for every instance of a
/// Relationship. The Scope is the exception: clause 4.5.6 represents it as a
/// temporal Property that "can only have the non-reified Temporal Properties
/// createdAt, modifiedAt, deletedAt and observedAt as sub-Properties", so an
/// instance of the Scope has no `instanceId` to keep, render or address.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum EvolutionInstance {
    /// An instance of an Attribute, identified by its `instanceId`.
    Attribute(InstanceIdentity),
    /// An instance of the Scope, which carries no identity of its own.
    Scope,
}
