//! How a request asks for one page of a collection, and the one policy that
//! decides which page that is (clauses 4.12, 5.5.9 and 6.3.10).
//!
//! Clause 4.12 leaves the pagination mechanism to the broker, clause 5.5.9.1
//! permits the pointers between pages to be "implemented in a completely
//! opaque way", and clause 5.5.9.2 adds the transparent `limit`/`offset` pair
//! as an option beside them. Every paged collection reads the same three
//! parameters ([`parameters::PageParameters`]) and hands them, with what the
//! collection itself contributes ([`page_request::PageRequest`]), to the
//! operator-configured [`policy::PaginationPolicy`], which alone decides the
//! [`andromeda_ngsi_ld_query::page_window::PageWindow`] the stores read.

pub mod cursor_request;
pub mod page_context;
pub mod page_pointers;
pub mod page_request;
pub mod page_size;
pub mod parameters;
pub mod policy;
