//! The broker's configuration schema.
//!
//! Every configuration section, its mode enums, its defaults and the value
//! types they are built from live here, so naming, nesting and completeness
//! are decided in one place. The crate is the bottom of the in-tree graph: it
//! names no other workspace crate, and a domain crate takes the one section it
//! reads, never the whole configuration.
//!
//! A value that carries a unit, a duration or a byte size, renders and parses
//! in the spelling a human writes (`"30s"`, `"7days"`, `"64 MiB"`) rather than
//! a bare integer whose unit lives only in the field name.

pub mod address_rule;
pub mod broker_config;
mod built_in_defaults;
pub mod comma_list;
pub mod header_section_limit;
pub mod human_byte_size;
pub mod human_duration;
pub mod load;
pub mod mqtt_packet_limit;
pub mod notification_body_limit;
pub mod redact;
pub mod reference;
pub mod schema;
pub mod validate;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["figment2"],
}
