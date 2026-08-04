//! Prelude re-exports consumed by every test binary.
//!
//! ```ignore
//! use andromeda_test_common::prelude::*;
//! use pretty_assertions::{assert_eq, assert_ne};
//! ```
//!
//! The second `use` is required on every test file: Rust's macro namespace
//! does not let a `pub use` re-export shadow the `assert_eq!` / `assert_ne!`
//! macros that the `std` prelude injects. An explicit named import does
//! take precedence, so each file spells out the `pretty_assertions` import
//! to route the short names to the coloured-diff variants.

pub use crate::{
    assert::{assert_entity_id, assert_problem_details},
    client::{
        NgsiLdClient,
        NgsiLdResponse,
        entity::{require_location, results_count},
    },
    config::{BASE_URL, COMPOUND_CONTEXT, CORE_CONTEXT, DATABASE_URL, TEST_SUITE_CONTEXT, callback_url},
    error,
    header::{jsonld_link_header, warning_header_values},
    ids,
    mock::{ContextSourceMock, MqttReceiver, NotificationReceiver},
    scope::TestScope,
};
