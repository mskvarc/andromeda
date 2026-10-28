//! Batch entity operations: several entities sent in one request, each reported on its own (clauses 5.6.7 to 5.6.10,
//! 5.6.17).

mod create_batch_of_entities;
mod delete_batch_of_entities;
mod merge_batch_of_entities;
mod update_batch_of_entities;
mod upsert_batch_of_entities;
