//! What the Serve @context operation answers with (clause 5.13.4).

use andromeda_jsonld::loader::cache_lifetime::CacheLifetime;
use serde_json::Value;

/// One answer of the Serve @context operation.
///
/// The three answers differ in how long a party holding one may keep it, which
/// is why they are told apart here rather than left as one JSON value: only a
/// document the broker stores itself can be changed or deleted behind the
/// holder's back, by this process or another sharing the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServedContext {
    /// The lifecycle metadata of an entry (`details=true`, clause 5.13.4.4).
    Metadata(Value),
    /// An embedded NGSI-LD core context, which no operation can change.
    Embedded(Value),
    /// A clause 5.13.1 `"Hosted"` or `"ImplicitlyCreated"` document.
    Stored {
        /// The stored document.
        document: Value,
        /// How long a holder may use its copy before asking again: the span
        /// after which this broker reads the stored row again itself.
        revalidation: CacheLifetime,
    },
}
