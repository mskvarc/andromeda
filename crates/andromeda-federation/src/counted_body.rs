//! A write payload that counts its serializations, for the tests proving a
//! forwarded body is serialised only once a registration needs it.

use serde::{Serialize, Serializer};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A write payload that counts how often it is serialised.
pub(crate) struct CountedBody {
    /// The payload.
    document: Value,
    /// How many times it was serialised.
    serialisations: AtomicUsize,
}

impl CountedBody {
    /// A payload serialising to `document`, not serialised yet.
    pub(crate) const fn new(document: Value) -> CountedBody {
        CountedBody {
            document,
            serialisations: AtomicUsize::new(0),
        }
    }

    /// How many times the payload was serialised so far.
    pub(crate) fn serialisations(&self) -> usize {
        self.serialisations.load(Ordering::SeqCst)
    }
}

// Written by hand because the count is a side effect no derive can add.
impl Serialize for CountedBody {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.serialisations.fetch_add(1, Ordering::SeqCst);
        self.document.serialize(serializer)
    }
}
