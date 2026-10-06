//! The running count of one geometry.

use crate::geometry_census::{GeometryCensusError, GeometryOverLimit, GeometryPositionLimit, PositionCount};
use andromeda_ngsi_ld_number::json_value::value_census::ValueTally;
use serde::de::{DeserializeSeed, Error as DeserializeError};
use serde_json::Deserializer as JsonReader;
use std::cell::Cell;

/// The running count of one geometry.
pub(crate) struct Census<'a> {
    /// Positions counted so far.
    positions: Cell<usize>,
    /// The most positions the geometry may hold.
    position_limit: GeometryPositionLimit,
    /// Whether a position passed the limit, which tells that abort apart from
    /// one the value tally or the reader raised.
    positions_exceeded: Cell<bool>,
    /// The tally every other value is counted into.
    pub(crate) values: &'a ValueTally,
}

impl<'a> Census<'a> {
    /// A census of no positions under `position_limit`, counting other values
    /// into `values`.
    pub(crate) const fn new(position_limit: GeometryPositionLimit, values: &'a ValueTally) -> Census<'a> {
        Census {
            positions: Cell::new(0),
            position_limit,
            positions_exceeded: Cell::new(false),
            values,
        }
    }

    /// The positions counted.
    pub(crate) const fn positions(&self) -> PositionCount {
        PositionCount::new(self.positions.get())
    }

    /// Count one position inside a streamed walk.
    pub(crate) fn stream_position<E: DeserializeError>(&self) -> Result<(), E> {
        let counted = self.positions.get().saturating_add(1);
        self.positions.set(counted);
        if counted > self.position_limit.get() {
            self.positions_exceeded.set(true);
            return Err(E::custom("the geometry position limit is exceeded"));
        }
        Ok(())
    }

    /// The refusal a count past one of the limits gives.
    pub(crate) const fn refusal_past_a_limit(&self) -> GeometryCensusError {
        GeometryCensusError::OverLimit(if self.positions_exceeded.get() {
            GeometryOverLimit::Positions { limit: self.position_limit }
        } else {
            GeometryOverLimit::Values { limit: self.values.limit() }
        })
    }

    /// Stream `text` through `seed`, which reads one JSON value.
    pub(crate) fn stream<'de, S: DeserializeSeed<'de, Value = ()>>(&self, text: &'de str, seed: S) -> Result<PositionCount, GeometryCensusError> {
        let mut reader = JsonReader::from_str(text);
        match seed.deserialize(&mut reader).and_then(|()| reader.end()) {
            Ok(()) => Ok(self.positions()),
            Err(_aborted) if self.positions_exceeded.get() || self.values.exceeded() => Err(self.refusal_past_a_limit()),
            Err(error) => Err(GeometryCensusError::Syntax(error)),
        }
    }
}
