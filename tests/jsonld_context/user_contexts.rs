//! The user `@context` documents the rendering cases name.
//!
//! Each shape stands for one row of the rendering rule:
//! what the last context a document leaves
//! in force is decides whether a response names the v1.9 core `@context` after
//! it (clauses 5.2.3 and 4.4) or names the document alone (Annex C.8). Every
//! document defines, directly or through the one it references, the two terms
//! the cases write: the `Device` type and its `speed` Property.

use crate::{
    context_origin::ContextOrigin,
    core_contexts::{CORE_V18, core},
};
use serde_json::{Value, json};

/// The IRI every document maps the `Device` type to.
pub const DEVICE_IRI: &str = "https://example.org/terms/Device";

/// The IRI every document but [`UserContexts::second`] maps `speed` to.
pub const SPEED_IRI: &str = "https://example.org/terms/speed";

/// The IRI [`UserContexts::second`] maps `speed` to.
pub const OTHER_SPEED_IRI: &str = "https://example.org/other/speed";

/// The `@vocab` [`UserContexts::vocab_after_core`] sets after the core
/// `@context`.
pub const USER_VOCAB: &str = "https://example.org/vocab/";

/// The two term definitions every document carries or reaches.
pub fn term_definitions() -> Value {
    json!({ "Device": DEVICE_IRI, "speed": SPEED_IRI })
}

/// Every user `@context` shape, served from one origin.
pub struct UserContexts {
    origin: ContextOrigin,
    /// The two terms, and no core `@context`.
    pub terms: String,
    /// `terms` followed by the v1.9 core `@context`: the ETSI compound shape,
    /// which ends with the core `@context`.
    pub wrapper: String,
    /// The v1.9 core `@context` first, then the two terms inline, so its own
    /// definitions are the last ones in force.
    pub importing: String,
    /// `terms`, the v1.9 core `@context`, then a `@vocab` of its own, which JSON-LD
    /// protection does not cover.
    pub vocab_after_core: String,
    /// `terms` followed by the protected v1.8 core `@context`.
    pub v18_tail: String,
    /// One definition importing the v1.8 core `@context` beside the two terms.
    pub imports_v18: String,
    /// A document naming [`Self::wrapper`] alone: a chain that ends with the
    /// core `@context`.
    pub outer: String,
    /// A document whose last entry is a relative reference to a sibling that
    /// ends with the v1.9 core `@context`.
    pub relative: String,
    /// `speed` mapped to [`OTHER_SPEED_IRI`] and `Device` as everywhere else.
    pub second: String,
}

impl UserContexts {
    /// Start an origin serving every shape.
    pub async fn start() -> UserContexts {
        let origin = ContextOrigin::start().await;
        let terms = origin.serve_document("/user/terms.jsonld", &json!({ "@context": term_definitions() })).await;
        let wrapper = origin.serve_document("/user/wrapper.jsonld", &json!({ "@context": [terms, core()] })).await;
        let importing = origin
            .serve_document("/user/importing.jsonld", &json!({ "@context": [core(), term_definitions()] }))
            .await;
        let vocab_after_core = origin
            .serve_document(
                "/user/vocab-after-core.jsonld",
                &json!({ "@context": [terms, core(), { "@vocab": USER_VOCAB }] }),
            )
            .await;
        let v18_tail = origin.serve_document("/user/v18-tail.jsonld", &json!({ "@context": [terms, CORE_V18] })).await;
        let mut import_definition = term_definitions();
        import_definition["@import"] = json!(CORE_V18);
        let imports_v18 = origin
            .serve_document("/user/imports-v18.jsonld", &json!({ "@context": import_definition }))
            .await;
        let outer = origin.serve_document("/user/outer.jsonld", &json!({ "@context": wrapper })).await;
        origin
            .serve_document("/user/nested/wrapped.jsonld", &json!({ "@context": [terms, core()] }))
            .await;
        let relative = origin
            .serve_document(
                "/user/nested/relative.jsonld",
                &json!({ "@context": [{ "rate": "https://example.org/terms/rate" }, "wrapped.jsonld"] }),
            )
            .await;
        let second = origin
            .serve_document(
                "/user/second.jsonld",
                &json!({ "@context": { "Device": DEVICE_IRI, "speed": OTHER_SPEED_IRI } }),
            )
            .await;
        UserContexts {
            origin,
            terms,
            wrapper,
            importing,
            vocab_after_core,
            v18_tail,
            imports_v18,
            outer,
            relative,
            second,
        }
    }

    /// The origin, for the cases that serve more documents from it.
    pub const fn origin(&self) -> &ContextOrigin {
        &self.origin
    }

    /// Stop serving.
    pub async fn shutdown(self) {
        self.origin.shutdown().await;
    }
}
