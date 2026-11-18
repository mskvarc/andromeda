//! Deciding a Query Term once per entity state across the Subscriptions that
//! share it.
//!
//! The Subscriptions of a tenant repeat terms: a thousand of them may hold
//! `speed>50` among their conjuncts. Compiling a Subscription gives each of
//! its distinct terms a small integer from the tenant's [`TermInterner`], and
//! a matching pass over one entity state keeps in a [`TermMemo`] the verdict
//! of every term it has decided, so a term shared by a thousand Subscriptions
//! is evaluated once per state.
//!
//! A number is held through a [`TermNumber`], which every filter numbered with
//! it shares. When the last of them is dropped the number goes back to the
//! interner and a later term takes it, so the numbers in use never exceed the
//! distinct terms of the Subscriptions alive at once. A matching pass holds the
//! Subscriptions it decides, so no number it reads changes hands during it.

use andromeda_ngsi_ld_query::filter::reading::term_reading::TermReading;
use std::{
    collections::BTreeMap,
    ptr,
    sync::{
        Arc,
        Mutex,
        PoisonError,
        Weak,
        atomic::{AtomicU64, Ordering},
    },
};

/// The tokens handed out, one per interner.
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(0);

/// Which interner numbered a term, so a memo is only ever read with the
/// numbers it was made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InternerToken(u64);

/// The small integer a distinct term has among the live terms of one
/// interner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermId(usize);

/// The number one distinct term holds for as long as a filter numbered with
/// it is alive.
#[derive(Debug)]
pub struct TermNumber {
    /// The number.
    id: TermId,
    /// The term it numbers, the key it is released under.
    term: TermReading,
    /// The numbering it returns to when dropped, gone once the interner is.
    numbering: Weak<Mutex<Numbering>>,
}

impl TermNumber {
    /// The number.
    #[must_use]
    pub const fn id(&self) -> TermId {
        self.id
    }
}

// Hand-written because dropping the last handle on a number gives the number
// back to its interner, a release no derive can express.
impl Drop for TermNumber {
    fn drop(&mut self) {
        if let Some(numbering) = self.numbering.upgrade() {
            numbering.lock().unwrap_or_else(PoisonError::into_inner).release(self);
        }
    }
}

/// The live numbers of one interner and the numbers free to hand out.
///
/// One lock guards the map and the free list together, since a number moves
/// between them in one step.
#[derive(Debug, Default)]
struct Numbering {
    /// The number each live term holds.
    by_term: BTreeMap<TermReading, Weak<TermNumber>>,
    /// Numbers released by terms no filter holds any more.
    free: Vec<TermId>,
    /// The number after the highest handed out.
    next: usize,
}

impl Numbering {
    /// The number `term` holds, `None` when no filter holds one.
    fn held(&self, term: &TermReading) -> Option<Arc<TermNumber>> {
        self.by_term.get(term).and_then(Weak::upgrade)
    }

    /// A number no live term holds.
    fn unused(&mut self) -> TermId {
        self.free.pop().unwrap_or_else(|| {
            let id = TermId(self.next);
            self.next += 1;
            id
        })
    }

    /// Takes back the number `released` held. A term numbered again while
    /// `released` was being dropped holds a number of its own, which stays.
    fn release(&mut self, released: &TermNumber) {
        if self.by_term.get(&released.term).is_some_and(|held| ptr::eq(held.as_ptr(), released)) {
            self.by_term.remove(&released.term);
        }
        self.free.push(released.id);
    }
}

/// The numbers of the distinct terms of the Subscriptions of one tenant.
#[derive(Debug)]
pub struct TermInterner {
    /// This interner among all.
    token: InternerToken,
    /// The numbers in use and free.
    numbering: Arc<Mutex<Numbering>>,
}

impl TermInterner {
    /// An interner that has numbered no term.
    #[must_use]
    pub fn new() -> TermInterner {
        TermInterner {
            token: InternerToken(NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)),
            numbering: Arc::new(Mutex::new(Numbering::default())),
        }
    }

    /// This interner among all.
    #[must_use]
    pub const fn token(&self) -> InternerToken {
        self.token
    }

    /// The number of `term`: the one a live term equal to it holds, else one
    /// no live term holds.
    pub fn number(&self, term: &TermReading) -> Arc<TermNumber> {
        let mut numbering = self.numbering.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(held) = numbering.held(term) {
            return held;
        }
        let number = Arc::new(TermNumber {
            id: numbering.unused(),
            term: term.clone(),
            numbering: Arc::downgrade(&self.numbering),
        });
        numbering.by_term.insert(term.clone(), Arc::downgrade(&number));
        number
    }
}

// Hand-written because every interner draws a token of its own from the
// counter, which a derive cannot.
impl Default for TermInterner {
    fn default() -> TermInterner {
        TermInterner::new()
    }
}

/// What a memo holds for one term.
#[derive(Debug, Clone, Copy)]
struct Slot {
    /// The pass the verdict was decided in.
    pass: u32,
    /// The verdict.
    verdict: bool,
}

/// The verdicts of the terms decided in the current pass over one entity
/// state, for the numbers of one interner.
///
/// A pass starts by counting: a verdict stamped with an earlier pass is not
/// one. The memo is exclusive to the pass using it, so none of its state is
/// shared between threads.
#[derive(Debug)]
pub struct TermMemo {
    /// The interner whose numbers the memo is read with.
    token: InternerToken,
    /// The pass in progress, never zero.
    pass: u32,
    /// The verdict by term number.
    slots: Vec<Slot>,
}

impl TermMemo {
    /// An empty memo for the numbers of `interner`, with a pass begun.
    #[must_use]
    pub const fn new(interner: &TermInterner) -> TermMemo {
        TermMemo {
            token: interner.token,
            pass: 1,
            slots: Vec::new(),
        }
    }

    /// The interner whose numbers the memo is read with.
    #[must_use]
    pub const fn token(&self) -> InternerToken {
        self.token
    }

    /// Starts a pass over another entity state: no verdict of an earlier pass
    /// stands.
    pub fn begin_pass(&mut self) {
        self.pass = self.pass.wrapping_add(1);
        if self.pass == 0 {
            self.slots.clear();
            self.pass = 1;
        }
    }

    /// The verdict of the term numbered `id` in this pass, decided by `decide`
    /// when no earlier use of it did.
    pub fn verdict(&mut self, id: TermId, decide: impl FnOnce() -> bool) -> bool {
        if self.slots.len() <= id.0 {
            self.slots.resize(id.0 + 1, Slot { pass: 0, verdict: false });
        }
        let pass = self.pass;
        match self.slots.get_mut(id.0) {
            Some(slot) if slot.pass == pass => slot.verdict,
            Some(slot) => {
                let verdict = decide();
                *slot = Slot { pass, verdict };
                verdict
            }
            None => decide(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::q::term_memo::{TermInterner, TermMemo};
    use andromeda_ngsi_ld_query::filter::{
        Filter,
        language_matching::LanguageMatching,
        reading::{filter_reading::FilterReading, term_reading::TermReading},
    };

    /// The reading of the one term `q` is.
    fn term(q: &str) -> Box<TermReading> {
        match FilterReading::of(&q.parse::<Filter>().expect("a clause 4.9 filter"), LanguageMatching::TagEquality) {
            FilterReading::Term(term) => term,
            FilterReading::All(_) | FilterReading::Any(_) => panic!("{q} is one term"),
        }
    }

    #[test]
    fn equal_terms_share_a_number_and_distinct_terms_differ() {
        let interner = TermInterner::new();
        let first = interner.number(&term("a==1"));
        assert_eq!(interner.number(&term("a==1")).id(), first.id());
        assert_ne!(interner.number(&term("a==2")).id(), first.id());
    }

    /// A number no filter holds any more is handed to the next new term, so
    /// churned Subscriptions leave no numbers behind.
    #[test]
    fn a_released_number_is_handed_to_the_next_new_term() {
        let interner = TermInterner::new();
        let kept = interner.number(&term("a==1"));
        let released = interner.number(&term("a==2")).id();
        let reused = interner.number(&term("a==3"));
        assert_eq!(reused.id(), released);
        assert_ne!(reused.id(), kept.id());
    }

    /// A term numbered again after its number was released holds a number of
    /// its own, distinct from every live one.
    #[test]
    fn a_term_numbered_again_after_release_takes_a_live_number() {
        let interner = TermInterner::new();
        let other = interner.number(&term("b==1"));
        drop(interner.number(&term("a==1")));
        let again = interner.number(&term("a==1"));
        assert_ne!(again.id(), other.id());
        assert_eq!(interner.number(&term("a==1")).id(), again.id());
    }

    #[test]
    fn a_term_is_decided_once_per_pass() {
        let interner = TermInterner::new();
        let number = interner.number(&term("a==1"));
        let id = number.id();
        let mut memo = TermMemo::new(&interner);
        let mut decisions = 0;
        for _ in 0..3 {
            assert!(memo.verdict(id, || {
                decisions += 1;
                true
            }));
        }
        assert_eq!(decisions, 1);
        memo.begin_pass();
        assert!(!memo.verdict(id, || {
            decisions += 1;
            false
        }));
        assert_eq!(decisions, 2);
    }

    #[test]
    fn interners_have_their_own_tokens() {
        assert_ne!(TermInterner::new().token(), TermInterner::new().token());
    }
}
