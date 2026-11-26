//! The rows a v1.0.0 tenant stored, written as that release wrote them, the
//! reads that hold the upgraded rows to what the migrations promise, and the
//! registry and catalogue the tenant template is checked through.

pub(crate) mod census_ledger;
pub(crate) mod columnstore;
pub(crate) mod current_state;
pub(crate) mod database_catalog;
pub(crate) mod deliveries;
pub(crate) mod history;
pub(crate) mod ledgers;
pub(crate) mod observed;
pub(crate) mod queues;
pub(crate) mod registrations;
pub(crate) mod registry;
pub(crate) mod rehearsal;
pub(crate) mod server_settings;
pub(crate) mod snapshot_records;
pub(crate) mod statements;
pub(crate) mod test_error;
pub(crate) mod vocabulary;
