//! The rows a v1.0.0 tenant stored, written as that release wrote them, and
//! the reads that hold the upgraded rows to what the migrations promise.

pub(crate) mod census_ledger;
pub(crate) mod columnstore;
pub(crate) mod current_state;
pub(crate) mod history;
pub(crate) mod ledgers;
pub(crate) mod observed;
pub(crate) mod queues;
pub(crate) mod rehearsal;
pub(crate) mod snapshot_records;
pub(crate) mod statements;
pub(crate) mod test_error;
pub(crate) mod vocabulary;
