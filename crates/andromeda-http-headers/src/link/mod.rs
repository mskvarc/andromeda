//! SPDX-FileCopyrightText: 2014-2025 Sean `McArthur`
//! SPDX-License-Identifier: MIT
//!
//! Typed `Link` header for NGSI-LD (RFC 8288, ETSI GS CIM 009 clauses 6.3.5, 6.3.6, 6.3.10).
//!
//! This module is derived from the disabled `Link` implementation in
//! <https://github.com/hyperium/headers>.
//! This module, including local modifications, is distributed under the MIT License.
//!
//! Implements the subset of the HTTP `Link` header
//! ([RFC 8288](https://datatracker.ietf.org/doc/html/rfc8288)) required by NGSI-LD:
//!
//! - **JSON-LD `@context` links** (clause 6.3.5): `rel="http://www.w3.org/ns/json-ld#context"`
//!   with `type="application/ld+json"`, used to associate a JSON-LD context with requests
//!   and responses.
//! - **Pagination links** (clause 6.3.10): `rel="next"` / `rel="prev"` with a `type`
//!   parameter matching the negotiated media type.
//! - **Notification context links** (clauses 6.3.8, 6.3.9): same structure as `@context`
//!   links, included in HTTP notification POST requests.
//! - **Version profile links** (clause 6.3.21): `rel="profile"` or a `profile` attribute
//!   naming `https://uri.etsi.org/ngsi-ld/profile/<major>.<minor>`.
//!
//! Only the `rel`, `type` and `profile` link-param attributes are supported. Other
//! RFC 8288 parameters (`anchor`, `rev`, `hreflang`, `media`, `title`, `title*`) are not
//! used by NGSI-LD and are silently ignored during parsing for interoperability.

pub mod error;
pub mod header;
pub mod profile;
pub mod quoting;
pub mod relation_type;
pub mod value;
