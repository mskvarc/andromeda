//! The conversion of data-integrity seals when a v1.0.0 tenant upgrades, as
//! the seal documentation and the migrations' own headers state it.

mod columnstore_history;
mod conversion_functions;
mod edge_seals;
mod queued_seals;
mod seal_free_database;
mod top_level_seal;
mod typed_seal;
mod unconvertible_seal;
