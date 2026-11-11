//! The XSD datatype the W3C Data Integrity v2 `@context` coerces the `created`
//! of a proof to (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2).
//!
//! ETSI TS 104 179 V1.1.1 clause 5.1.2 holds `created` to the NGSI-LD
//! `DateTime` form, which every `xsd:dateTime` it admits also is, and the
//! broker writes it back typed as the context types it.

use iri_rs::Iri;
use xsd_rs::XSD_DATE_TIME;

/// The IRI of `xsd:dateTime`, the datatype a proof's `created` is coerced to.
pub const PROOF_CREATED_DATATYPE: Iri<&'static str> = XSD_DATE_TIME;
