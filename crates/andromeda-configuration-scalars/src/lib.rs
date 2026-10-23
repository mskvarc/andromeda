//! Human-readable scalar newtypes shared by every Andromeda configuration
//! section.
//!
//! A configuration value that carries a unit -- a duration or a byte size --
//! renders and parses in the spelling a human writes (`"30s"`, `"7days"`,
//! `"64 MiB"`) rather than a bare integer whose unit lives only in the field
//! name. This crate owns the two newtypes that carry those spellings so every
//! section shares one representation and the `bytesize` and `humantime-serde`
//! dependencies are declared in exactly one place, and the request header
//! section limit both HTTP listeners enforce, a byte size bounded to what every
//! HTTP version enforces identically.

pub mod header_section_limit;
pub mod human_byte_size;
pub mod human_duration;
