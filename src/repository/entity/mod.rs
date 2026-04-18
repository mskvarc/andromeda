//! Persistence layer for NGSI-LD entities.

pub mod mapper;
pub mod params;
pub mod repo;
pub mod rows;

pub(crate) use params::{PreparedEntityRow, decompose_rows};
