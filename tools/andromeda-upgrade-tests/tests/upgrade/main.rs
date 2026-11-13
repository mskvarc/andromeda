//! Upgrade rehearsals of a v1.0.0 tenant.
//!
//! Each test builds a database at the schema v1.0.0 sealed, writes the rows
//! that release stored, applies the migrations this build embeds on top, and
//! reads the result back. The expected behaviour is the one the
//! documentation and the migrations' own headers state.

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod lockstep;
#[cfg(test)]
mod seal;
#[cfg(test)]
mod tenancy;
