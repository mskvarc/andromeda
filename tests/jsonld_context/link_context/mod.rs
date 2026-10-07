//! The one context `Link` an `application/json` response carries.
//!
//! Clause 6.3.6: an `application/json` response "shall include a Link to the
//! associated JSON-LD @context"; JSON-LD 1.1 section 6.1: "A response MUST NOT
//! contain more than one HTTP Link Header using the
//! <http://www.w3.org/ns/json-ld#context> link relation". The link names the value
//! an `application/ld+json` body would carry: one URL names itself, anything
//! else is a wrapper the broker hosts as an `ImplicitlyCreated` `@context`
//! (clause 5.13.1, Annex C.8) under an absolute URL. A
//! response without a body, a 207 and an error name no context at all.

mod advertised_url;
mod carriers;
mod hosted_wrapper;
mod hosting_failure;
mod rehosting;
mod single_reference;
mod tenant_urls;

use crate::context_links::context_link_targets;
use reqwest::header::HeaderMap;

/// Assert `headers` carry no JSON-LD context link.
fn assert_no_context_link(headers: &HeaderMap, why: &str) {
    let targets = context_link_targets(headers);
    assert!(targets.is_empty(), "{why}: no context link, got {targets:?}");
}
