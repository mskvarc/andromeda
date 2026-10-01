//! The running count of one geometry, and its walk over a compact tree.

use crate::geometry_census::{COORDINATES_MEMBER, GeometryCensusError, GeometryOverLimit, GeometryPositionLimit, PositionCount};
use andromeda_ngsi_ld_number::json_value::value_census::ValueTally;
use serde::de::{DeserializeSeed, Error as DeserializeError};
use serde_json::{Deserializer as JsonReader, Value};
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

/// A count passed one of the census's limits.
#[derive(Debug)]
pub(crate) struct LimitPassed;

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

    /// Count one position.
    pub(crate) fn position(&self) -> Result<(), LimitPassed> {
        let counted = self.positions.get().saturating_add(1);
        self.positions.set(counted);
        if counted > self.position_limit.get() {
            self.positions_exceeded.set(true);
            return Err(LimitPassed);
        }
        Ok(())
    }

    /// Count one position inside a streamed walk.
    pub(crate) fn stream_position<E: DeserializeError>(&self) -> Result<(), E> {
        self.position().map_err(|LimitPassed| E::custom("the geometry position limit is exceeded"))
    }

    /// Count one value that is not part of a position.
    pub(crate) fn value(&self) -> Result<(), LimitPassed> {
        self.values.admit_one().map_err(|_exceeded| LimitPassed)
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

    /// Count a geometry object: its `coordinates` as coordinates, every
    /// other member as values.
    pub(crate) fn geometry_tree(&self, geometry: &Value) -> Result<(), LimitPassed> {
        let Value::Object(members) = geometry else {
            return self.values_tree(geometry);
        };
        self.value()?;
        members.iter().try_for_each(|(member, value)| {
            if member == COORDINATES_MEMBER {
                self.node_tree(value)
            } else {
                self.values_tree(value)
            }
        })
    }

    /// Count a coordinate node: a list or a position when it is an array,
    /// values otherwise.
    pub(crate) fn node_tree(&self, node: &Value) -> Result<(), LimitPassed> {
        let Value::Array(items) = node else {
            return self.values_tree(node);
        };
        match items.split_first() {
            None => self.value(),
            Some((Value::Array(_), _)) => {
                self.value()?;
                items.iter().try_for_each(|item| self.node_tree(item))
            }
            Some(_) => {
                self.position()?;
                items.iter().try_for_each(|ordinate| self.ordinate_tree(ordinate))
            }
        }
    }

    /// Count one member of a position: an ordinate is part of the position,
    /// anything that holds values counts them.
    pub(crate) fn ordinate_tree(&self, ordinate: &Value) -> Result<(), LimitPassed> {
        match ordinate {
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
            Value::Array(_) | Value::Object(_) => self.values_tree(ordinate),
        }
    }

    /// Count `value` and every value below it.
    pub(crate) fn values_tree(&self, value: &Value) -> Result<(), LimitPassed> {
        self.value()?;
        match value {
            Value::Array(items) => items.iter().try_for_each(|item| self.values_tree(item)),
            Value::Object(members) => members.values().try_for_each(|member| self.values_tree(member)),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
        }
    }
}
