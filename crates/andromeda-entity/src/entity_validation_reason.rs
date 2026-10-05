//! The concrete reasons an entity operation is rejected as `BadRequestData`.
//!
//! Every client-facing validation failure of entity policy is one variant here,
//! carrying the offending value as a typed field and chaining the typed source
//! error where one exists, so [`EntityError::Validation`](crate::error::EntityError::Validation)
//! never holds an ad-hoc string. Each `#[error]` renders lowercase client prose;
//! spec clause numbers stay in these doc comments, never in the message.

use crate::merge_fragment::MergeFragmentError;
use andromeda_context_commit::submission_refusal::SubmissionRefusal;
use andromeda_ngsi_ld_entity::{operation::mutation_error::MutationError, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_query::ordering::collation::IcuCollation;
use thiserror::Error;

/// Why an entity payload or operation is invalid.
#[derive(Debug, Error)]
pub enum EntityValidationReason {
    /// An Attribute member name of a compacted entity does not satisfy the
    /// attribute-name grammar.
    #[error("invalid attribute member name: {0}")]
    AttributeMemberName(#[source] NgsiLdReferenceError),
    /// An expanded attribute IRI does not satisfy the attribute-name grammar.
    #[error("invalid attribute name: {0}")]
    AttributeName(#[source] NgsiLdReferenceError),
    /// The written document is not one the entity model admits as a write
    /// (clause 5.5.4): an identifier, type, member, NGSI-LD Null, scope,
    /// `unitCode` or geometry the operation does not accept.
    ///
    /// Boxed: the refusal is the widest reason, and inlining it would enlarge
    /// every `Result` an entity operation returns.
    #[error(transparent)]
    Refused(Box<EntityRefusal>),
    /// The written instance cannot be applied to the stored entity: it
    /// declares another attribute family, or cannot become entity state.
    #[error(transparent)]
    Mutation(MutationError),
    /// An attribute name begins with the reserved `@` character.
    #[error("invalid attribute name '{name}': names beginning with '@' are reserved")]
    ReservedAttributeName {
        /// The rejected attribute name.
        name: AttributeName,
    },
    /// The short name the active `@context` declares for a requested attribute is
    /// not itself an NGSI-LD name.
    #[error("the context declares a short name for {requested} that is not an NGSI-LD name: {source}")]
    DeclaredName {
        /// The attribute the client requested.
        requested: AttributeName,
        /// The grammar violation.
        #[source]
        source: NgsiLdReferenceError,
    },
    /// `scope` was named in a partial-update, which cannot target it (clause 5.6.4).
    #[error("scope is not a partially updatable attribute")]
    ScopeNotPartiallyUpdatable,
    /// `scope` was named in an attribute replacement, which cannot target it.
    #[error("scope is not a replaceable attribute")]
    ScopeNotReplaceable,
    /// A merge fragment could not be adapted to the stored attribute's shape.
    #[error(transparent)]
    MemberAdaptation(#[from] MergeFragmentError),
    /// A decoded request could not be written back as the compact JSON the
    /// expansion reads.
    #[error("the request could not be serialized as JSON: {0}")]
    CompactDocument(#[source] serde_json::Error),
    /// A Replace Entity payload names an entity other than the one it replaces:
    /// clause 5.6.18.4 replaces the target "with the same Entity ID".
    #[error("the replacement names the entity '{named}', not the entity its URL names")]
    ReplacementNamesAnotherEntity {
        /// The entity the payload names.
        named: Identifier,
    },
    /// A distance ordering was requested with nothing to measure from (clause 5.7.2.4).
    #[error("distance-based ordering requires either a geo-query or an orderFrom parameter")]
    DistanceOrderingWithoutReference,
    /// The client's `collation` is not a locale the database can honour.
    #[error("'{collation}' does not conform to a valid ICU collation")]
    Collation {
        /// The rejected collation identifier.
        collation: IcuCollation,
    },
    /// A `noOverwrite` batch update named only attributes that already exist, so
    /// clause 5.6.9.4's local Append (clause 5.6.3) appended nothing and the
    /// Entity is not "successfully updated (Attributes appended)" for S (clause 5.6.9.5).
    #[error("no attributes were appended: every named attribute already exists and noOverwrite was set")]
    NoOverwriteConflict,
    /// The atomic commit port refused the submission as unusable (clause 5.5.6).
    #[error(transparent)]
    CommitRefused(SubmissionRefusal),
}
