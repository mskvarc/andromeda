//! HTTP header utilities for NGSI-LD API responses.

mod link;

pub use link::{insert_jsonld_contexts, jsonld_context_link, link_from_values, link_header_string, link_header_value, link_value};

pub use link::media_type;
