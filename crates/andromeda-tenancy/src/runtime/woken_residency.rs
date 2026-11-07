//! The place a runtime loaded for due work holds, and when it gives it up.
//!
//! A woken runtime is spared from being unloaded for its woken lifetime, so the
//! work it was loaded for runs first. While every place in the cache is taken,
//! the wake pass asks it whether that work has run, and a runtime whose work
//! has run gives up its place before the lifetime ends. A runtime still working
//! is asked again only after a wait that doubles with every answer, so a busy
//! runtime costs a handful of reads over its lifetime.

use crate::{
    due_work::{DueWork, DueWorkSource},
    runtime::process_epoch::{EPOCH, now_millis},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// The wait before a woken runtime still working is asked again, doubled for
/// every answer it has given since it was loaded.
const RECHECK_FLOOR: Duration = Duration::from_millis(100);

/// The doubling stops here: the wait after this many answers is the floor times
/// two to this power, which every woken lifetime under an hour bounds anyway.
const RECHECK_DOUBLINGS: u32 = 15;

/// The place a runtime the waker loaded for due work holds.
pub(super) struct WokenResidency {
    /// When the woken lifetime ends.
    end: Instant,
    /// Set once the runtime answered that its due work has run: from then on
    /// it holds no place a woken tenant could not take.
    released: AtomicBool,
    /// Milliseconds after [`EPOCH`] before which the runtime is not asked
    /// again; zero until its first answer.
    next_ask: AtomicU64,
    /// How many times it answered that its work is outstanding.
    outstanding_answers: AtomicU32,
    /// Where the runtime answers whether its due work has run.
    source: Arc<dyn DueWorkSource>,
}

impl WokenResidency {
    /// The place of a runtime loaded now for `lifetime`, asked through
    /// `source`, or `None` when the lifetime ends past what an `Instant` holds.
    pub(super) fn new(lifetime: Duration, source: Arc<dyn DueWorkSource>) -> Option<WokenResidency> {
        Instant::now().checked_add(lifetime).map(|end| WokenResidency {
            end,
            released: AtomicBool::new(false),
            next_ask: AtomicU64::new(0),
            outstanding_answers: AtomicU32::new(0),
            source,
        })
    }

    /// When the woken lifetime ends.
    pub(super) const fn end(&self) -> Instant {
        self.end
    }

    /// Whether the runtime still holds its place at `now`: within its lifetime
    /// and not released.
    pub(super) fn holds_place(&self, now: Instant) -> bool {
        now < self.end && !self.released.load(Ordering::Acquire)
    }

    /// Whether the runtime may be asked at `now` whether its work has run.
    pub(super) fn may_ask(&self, now: Instant) -> bool {
        self.holds_place(now) && millis_at(now) >= self.next_ask.load(Ordering::Acquire)
    }

    /// How long from `now` until the place may free: the lifetime's end, or
    /// the next time the runtime may be asked, whichever is sooner. `None` when
    /// it holds no place.
    pub(super) fn frees_in(&self, now: Instant) -> Option<Duration> {
        if !self.holds_place(now) {
            return None;
        }
        let lifetime_left = self.end.saturating_duration_since(now);
        let ask_in = Duration::from_millis(self.next_ask.load(Ordering::Acquire).saturating_sub(millis_at(now)));
        Some(lifetime_left.min(ask_in))
    }

    /// Ask the runtime whether its work has run, releasing its place when it
    /// has and putting off the next ask when it has not.
    pub(super) async fn ask(&self) -> DueWork {
        let answer = self.source.due_work().await;
        match answer {
            DueWork::Drained => self.released.store(true, Ordering::Release),
            DueWork::Outstanding => {
                let doublings = self.outstanding_answers.fetch_add(1, Ordering::AcqRel).min(RECHECK_DOUBLINGS);
                let wait = RECHECK_FLOOR.saturating_mul(1 << doublings);
                let wait_millis = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
                self.next_ask.store(now_millis().saturating_add(wait_millis), Ordering::Release);
            }
        }
        answer
    }
}

/// `at` in milliseconds after [`EPOCH`], zero for an instant before it.
fn millis_at(at: Instant) -> u64 {
    u64::try_from(at.saturating_duration_since(*EPOCH).as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use crate::{
        due_work::{DueWork, DueWorkSource},
        runtime::woken_residency::WokenResidency,
    };
    use async_trait::async_trait;
    use std::{
        sync::{
            Arc,
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    /// A source answering a settable verdict and counting the asks.
    struct Answering {
        /// What it answers.
        answer: Mutex<DueWork>,
        /// How often it was asked.
        asked: AtomicUsize,
    }

    #[async_trait]
    impl DueWorkSource for Answering {
        async fn due_work(&self) -> DueWork {
            self.asked.fetch_add(1, Ordering::SeqCst);
            self.answer.lock().map_or(DueWork::Outstanding, |answer| *answer)
        }
    }

    fn answering(answer: DueWork) -> Arc<Answering> {
        Arc::new(Answering {
            answer: Mutex::new(answer),
            asked: AtomicUsize::new(0),
        })
    }

    fn residency(lifetime: Duration, source: &Arc<Answering>) -> WokenResidency {
        WokenResidency::new(lifetime, Arc::clone(source) as Arc<dyn DueWorkSource>).expect("a lifetime an instant holds")
    }

    #[tokio::test]
    async fn a_runtime_whose_work_has_run_gives_up_its_place() {
        let source = answering(DueWork::Drained);
        let woken = residency(Duration::from_secs(60), &source);
        assert!(woken.holds_place(Instant::now()));

        assert_eq!(woken.ask().await, DueWork::Drained);

        assert!(!woken.holds_place(Instant::now()));
        assert!(!woken.may_ask(Instant::now()), "a released runtime is not asked again");
        assert_eq!(woken.frees_in(Instant::now()), None);
    }

    #[tokio::test]
    async fn a_runtime_still_working_keeps_its_place_and_is_asked_again_after_a_doubling_wait() {
        let source = answering(DueWork::Outstanding);
        let woken = residency(Duration::from_secs(60), &source);
        assert!(woken.may_ask(Instant::now()), "a runtime never asked may be asked at once");

        assert_eq!(woken.ask().await, DueWork::Outstanding);
        let first_wait = woken.frees_in(Instant::now()).expect("it holds its place");
        assert_eq!(woken.ask().await, DueWork::Outstanding);
        let second_wait = woken.frees_in(Instant::now()).expect("it holds its place");

        assert!(woken.holds_place(Instant::now()));
        assert!(!woken.may_ask(Instant::now()));
        assert!(
            first_wait > Duration::from_millis(50) && first_wait <= Duration::from_millis(100),
            "{first_wait:?}"
        );
        assert!(
            second_wait > Duration::from_millis(150) && second_wait <= Duration::from_millis(200),
            "{second_wait:?}"
        );
    }

    #[tokio::test]
    async fn the_wait_never_outlasts_the_woken_lifetime() {
        let source = answering(DueWork::Outstanding);
        let woken = residency(Duration::from_millis(150), &source);
        for _ in 0..4 {
            assert_eq!(woken.ask().await, DueWork::Outstanding);
        }

        let frees_in = woken.frees_in(Instant::now()).expect("it holds its place");

        assert!(frees_in <= Duration::from_millis(150), "{frees_in:?}");
    }

    #[test]
    fn a_runtime_past_its_woken_lifetime_holds_no_place() {
        let woken = residency(Duration::from_secs(60), &answering(DueWork::Outstanding));
        let after = woken.end() + Duration::from_millis(1);

        assert!(!woken.holds_place(after));
        assert!(!woken.may_ask(after));
        assert_eq!(woken.frees_in(after), None);
    }
}
