//! One item of a `ListRelationship` object list (clause 4.5.22).
//!
//! Clause 4.5.22.2 writes the normalized list as objects naming each target
//! (`[{"object": uri}]`), which expands to `{"hasObject": [{"@id": uri}]}`;
//! clause 4.5.22.3 writes the concise list as the bare targets, which expands
//! to `{"@id": uri}`, except under a context that does not type `objectList`
//! `@id` (the core context), where it expands to `{"@value": uri}`. All three
//! spellings occur in stored documents, and each item
//! is written back the way it was stored.

use andromeda_ngsi_ld_lexicon::uri::Uri;

/// One target of a `ListRelationship`, in its stored spelling.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ListObject {
    /// `{"@id": uri}`.
    Bare(Uri),
    /// `{"@value": uri}`.
    Literal(Uri),
    /// `{"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": uri}]}`.
    Wrapped(Uri),
}

impl ListObject {
    /// The target, whichever way it is spelled.
    #[must_use]
    pub const fn uri(&self) -> &Uri {
        match self {
            ListObject::Bare(uri) | ListObject::Literal(uri) | ListObject::Wrapped(uri) => uri,
        }
    }
}
