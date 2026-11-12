//! The conversion of data-integrity seals when a v1.0.0 tenant upgrades.
//!
//! Each test builds a database at the schema v1.0.0 sealed, writes the rows
//! that release stored, applies the migrations this build embeds on top, and
//! reads the result back. The expected behaviour is the one the seal
//! documentation and the migrations' own headers state.

#[cfg(test)]
mod census_ledger;
#[cfg(test)]
mod current_state;
#[cfg(test)]
mod history;
#[cfg(test)]
mod observed;
#[cfg(test)]
mod queued_seals;
#[cfg(test)]
mod queues;
#[cfg(test)]
mod rehearsal;
#[cfg(test)]
mod seal_free_database;
#[cfg(test)]
mod statements;
#[cfg(test)]
mod test_error;
#[cfg(test)]
mod top_level_seal;
#[cfg(test)]
mod typed_seal;
#[cfg(test)]
mod unconvertible_seal;
#[cfg(test)]
mod vocabulary;
