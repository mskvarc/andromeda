//! Where an attribute instance stands in the member it is written in, which
//! decides the clause 4.5.2.3 abbreviations of the concise representation.
//!
//! Concise drops the `type` member of every attribute whose kind the value
//! member already implies, and collapses an attribute that carries nothing but
//! a type and a value down to the value alone, both only where the result reads
//! back as the attribute that was written, by the reading of
//! [`andromeda_ngsi_ld_representation::concise_reading`]. The typed walk of
//! [`crate::entity_body`] states where an instance stands through
//! [`InstanceStanding`].

/// Where an attribute instance stands in the member it is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstanceStanding {
    /// The member's one instance.
    Alone,
    /// One of the instances of a multi-attribute, which clause 4.5.2.3 writes
    /// as "a JSON-LD array with such JSON-LD objects": each stays an object.
    AmongInstances,
}
