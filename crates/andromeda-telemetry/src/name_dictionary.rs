//! Metrics describing the per-tenant name dictionary caches.
//!
//! How often a name is answered from the cache, why cached names leave it, how
//! many it holds and how many names are inserted. No series carries a tenant
//! label: the entries gauge is the process-wide total over every loaded
//! tenant's cache, which each cache adjusts by what it holds.

use crate::{
    names::{
        LABEL_CAUSE,
        LABEL_DICTIONARY,
        LABEL_OUTCOME,
        NAME_DICTIONARY_ENTRIES,
        NAME_DICTIONARY_EVICTIONS_TOTAL,
        NAME_DICTIONARY_INSERTIONS_TOTAL,
        NAME_DICTIONARY_LOOKUPS_TOTAL,
    },
    sample_value::SampleValue,
};
use metrics::{counter, gauge};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use strum::IntoStaticStr;

/// Entries held by every loaded `attribute-names` cache in the process.
static ATTRIBUTE_NAME_ENTRIES: AtomicU64 = AtomicU64::new(0);

/// Entries held by every loaded `entity-type-names` cache in the process.
static ENTITY_TYPE_NAME_ENTRIES: AtomicU64 = AtomicU64::new(0);

/// The dictionary a series describes.
///
/// Rendered as the kebab-case `dictionary` label value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum NameDictionary {
    /// The dictionary of expanded attribute IRIs.
    AttributeNames,
    /// The dictionary of expanded entity type IRIs.
    EntityTypeNames,
}

impl NameDictionary {
    /// The process-wide entry total of this dictionary's caches.
    const fn entry_total(self) -> &'static AtomicU64 {
        match self {
            NameDictionary::AttributeNames => &ATTRIBUTE_NAME_ENTRIES,
            NameDictionary::EntityTypeNames => &ENTITY_TYPE_NAME_ENTRIES,
        }
    }
}

/// Whether the cache held a name it was asked for.
///
/// Rendered as the kebab-case `outcome` label value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
enum LookupOutcome {
    /// The cache answered the name.
    Hit,
    /// The name was asked of the dictionary table.
    Miss,
}

/// Why a cached name left the cache.
///
/// Rendered as the kebab-case `cause` label value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum DictionaryEviction {
    /// The cache was over its byte budget.
    Size,
    /// The name went unused for the idle timeout.
    Idle,
}

/// Count the names one cache consult answered and the names it missed.
pub fn record_lookups(dictionary: NameDictionary, hits: usize, misses: usize) {
    record_lookup_outcome(dictionary, LookupOutcome::Hit, hits);
    record_lookup_outcome(dictionary, LookupOutcome::Miss, misses);
}

/// Count `count` names of `dictionary` that ended with `outcome`.
fn record_lookup_outcome(dictionary: NameDictionary, outcome: LookupOutcome, count: usize) {
    if count == 0 {
        return;
    }
    let dictionary: &'static str = dictionary.into();
    let outcome: &'static str = outcome.into();
    // `usize` is at most 64 bits wide on every supported target.
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    counter!(NAME_DICTIONARY_LOOKUPS_TOTAL, LABEL_DICTIONARY => dictionary, LABEL_OUTCOME => outcome).increment(count);
}

/// Count one name evicted from a cache with its cause.
pub fn record_eviction(dictionary: NameDictionary, cause: DictionaryEviction) {
    let dictionary: &'static str = dictionary.into();
    let cause: &'static str = cause.into();
    counter!(NAME_DICTIONARY_EVICTIONS_TOTAL, LABEL_DICTIONARY => dictionary, LABEL_CAUSE => cause).increment(1);
}

/// Count `count` names newly inserted into `dictionary`.
pub fn record_inserted(dictionary: NameDictionary, count: u64) {
    if count == 0 {
        return;
    }
    let dictionary: &'static str = dictionary.into();
    counter!(NAME_DICTIONARY_INSERTIONS_TOTAL, LABEL_DICTIONARY => dictionary).increment(count);
}

/// One cache's share of the process-wide entries gauge.
///
/// The cache reports what it holds, the share moves the dictionary's total by
/// the difference and sets the gauge to the total, and dropping the share takes
/// its last report back out.
#[derive(Debug)]
pub struct DictionaryEntries {
    /// The dictionary whose total this share moves.
    dictionary: NameDictionary,
    /// The entry count this cache last reported, `None` before its first
    /// report so that a first report of zero still publishes the series.
    reported: Mutex<Option<u64>>,
}

impl DictionaryEntries {
    /// A share of `dictionary`'s total that holds no entries yet.
    #[must_use]
    pub const fn new(dictionary: NameDictionary) -> DictionaryEntries {
        DictionaryEntries {
            dictionary,
            reported: Mutex::new(None),
        }
    }

    /// Report that this cache holds `count` entries.
    ///
    /// Caches report on writes only, so the lock is never taken on a lookup.
    pub fn report(&self, count: u64) {
        let mut reported = self.reported.lock();
        let previous = *reported;
        if previous == Some(count) {
            return;
        }
        *reported = Some(count);
        self.publish(previous.unwrap_or(0), count);
    }

    /// Move the total from this share's `previous` count to `current` and set
    /// the gauge to the result.
    fn publish(&self, previous: u64, current: u64) {
        let total = self.dictionary.entry_total();
        let moved = |held: u64| held.saturating_sub(previous).saturating_add(current);
        // The closure never declines, so both arms carry the total it moved.
        let held = match total.try_update(Ordering::Relaxed, Ordering::Relaxed, |held| Some(moved(held))) {
            Ok(held) | Err(held) => moved(held),
        };
        let dictionary: &'static str = self.dictionary.into();
        gauge!(NAME_DICTIONARY_ENTRIES, LABEL_DICTIONARY => dictionary).set(SampleValue::from(held));
    }
}

impl Drop for DictionaryEntries {
    fn drop(&mut self) {
        if let Some(previous @ 1..) = self.reported.get_mut().take() {
            self.publish(previous, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        name_dictionary::{DictionaryEntries, DictionaryEviction, NameDictionary, record_eviction, record_inserted, record_lookups},
        names::{NAME_DICTIONARY_ENTRIES, NAME_DICTIONARY_EVICTIONS_TOTAL, NAME_DICTIONARY_INSERTIONS_TOTAL, NAME_DICTIONARY_LOOKUPS_TOTAL},
        recorder::TelemetryRecorder,
    };
    use std::{thread::sleep, time::Duration};

    #[test]
    fn dictionaries_and_causes_render_as_kebab_case_label_values() {
        for (dictionary, expected) in [
            (NameDictionary::AttributeNames, "attribute-names"),
            (NameDictionary::EntityTypeNames, "entity-type-names"),
        ] {
            let rendered: &'static str = dictionary.into();
            assert_eq!(rendered, expected);
        }
        for (cause, expected) in [(DictionaryEviction::Size, "size"), (DictionaryEviction::Idle, "idle")] {
            let rendered: &'static str = cause.into();
            assert_eq!(rendered, expected);
        }
    }

    #[test]
    fn a_hit_and_a_miss_each_move_the_lookup_counter() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            record_lookups(NameDictionary::AttributeNames, 2, 1);
            record_lookups(NameDictionary::EntityTypeNames, 0, 3);
        });
        let rendered = recorder.render();
        for series in [
            "{dictionary=\"attribute-names\",outcome=\"hit\"} 2",
            "{dictionary=\"attribute-names\",outcome=\"miss\"} 1",
            "{dictionary=\"entity-type-names\",outcome=\"miss\"} 3",
        ] {
            assert!(rendered.contains(&format!("{NAME_DICTIONARY_LOOKUPS_TOTAL}{series}")), "{rendered}");
        }
        assert!(!rendered.contains("dictionary=\"entity-type-names\",outcome=\"hit\""), "{rendered}");
        Ok(())
    }

    #[test]
    fn evictions_are_counted_by_cause_and_mints_by_dictionary() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            record_eviction(NameDictionary::AttributeNames, DictionaryEviction::Size);
            record_eviction(NameDictionary::AttributeNames, DictionaryEviction::Idle);
            record_eviction(NameDictionary::AttributeNames, DictionaryEviction::Idle);
            record_inserted(NameDictionary::EntityTypeNames, 1);
            record_inserted(NameDictionary::AttributeNames, 0);
        });
        let rendered = recorder.render();
        for series in [
            format!("{NAME_DICTIONARY_EVICTIONS_TOTAL}{{dictionary=\"attribute-names\",cause=\"size\"}} 1"),
            format!("{NAME_DICTIONARY_EVICTIONS_TOTAL}{{dictionary=\"attribute-names\",cause=\"idle\"}} 2"),
            format!("{NAME_DICTIONARY_INSERTIONS_TOTAL}{{dictionary=\"entity-type-names\"}} 1"),
        ] {
            assert!(rendered.contains(&series), "{rendered}");
        }
        assert!(
            !rendered.contains(&format!("{NAME_DICTIONARY_INSERTIONS_TOTAL}{{dictionary=\"attribute-names\"}}")),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn a_first_report_of_zero_publishes_the_series() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            let share = DictionaryEntries::new(NameDictionary::EntityTypeNames);
            share.report(0);
            let rendered = recorder.render();
            assert!(
                rendered.contains(&format!("{NAME_DICTIONARY_ENTRIES}{{dictionary=\"entity-type-names\"}} ")),
                "{rendered}"
            );
        });
        Ok(())
    }

    /// The only test that moves the attribute-names total, so no other test can
    /// race the values it asserts. The last value is still rendered past the
    /// idle timeout.
    #[test]
    fn the_entries_gauge_sums_every_cache_and_forgets_a_dropped_one() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached_with(Duration::from_millis(50), None)?;
        let gauge = |rendered: &str, value: u64| rendered.contains(&format!("{NAME_DICTIONARY_ENTRIES}{{dictionary=\"attribute-names\"}} {value}\n"));
        recorder.record_locally(|| {
            let first = DictionaryEntries::new(NameDictionary::AttributeNames);
            let second = DictionaryEntries::new(NameDictionary::AttributeNames);
            first.report(5);
            second.report(3);
            assert!(gauge(&recorder.render(), 8), "{}", recorder.render());
            first.report(2);
            assert!(gauge(&recorder.render(), 5), "{}", recorder.render());
            drop(second);
            assert!(gauge(&recorder.render(), 2), "{}", recorder.render());
            drop(first);
            assert!(gauge(&recorder.render(), 0), "{}", recorder.render());
        });
        sleep(Duration::from_millis(150));
        assert!(gauge(&recorder.render(), 0), "{}", recorder.render());
        Ok(())
    }
}
