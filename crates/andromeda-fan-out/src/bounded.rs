//! Starting one future per item, at most `limit` at once.

use futures::{Stream, StreamExt, stream};

/// Start `start` on every item of `items`, at most `limit` at once, yielding
/// each outcome as its future completes.
///
/// Items are started in order, each once fewer than `limit` futures are
/// running. A `limit` of zero runs nothing.
pub fn fan_out_unordered<I, C, F>(items: I, limit: usize, start: C) -> impl Stream<Item = F::Output> + Send
where
    I: IntoIterator,
    I::IntoIter: Send,
    C: FnMut(I::Item) -> F + Send,
    F: Future + Send,
    F::Output: Send,
{
    stream::iter(items.into_iter().map(start)).buffer_unordered(limit)
}

/// Start `start` on every item of `items`, at most `limit` at once, yielding
/// the outcomes in the order of their items.
///
/// An outcome that completes ahead of an earlier item's is held until that
/// one completes. A `limit` of zero runs nothing.
pub fn fan_out_ordered<I, C, F>(items: I, limit: usize, start: C) -> impl Stream<Item = F::Output> + Send
where
    I: IntoIterator,
    I::IntoIter: Send,
    C: FnMut(I::Item) -> F + Send,
    F: Future + Send,
    F::Output: Send,
{
    stream::iter(items.into_iter().map(start)).buffered(limit)
}

#[cfg(test)]
mod tests {
    use crate::bounded::{fan_out_ordered, fan_out_unordered};
    use futures::{StreamExt, future::ready};
    use pretty_assertions::assert_eq;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    use tokio::{sync::Barrier, time::sleep};

    #[tokio::test]
    async fn an_ordered_fan_out_yields_the_outcomes_in_item_order() {
        let outcomes: Vec<u64> = fan_out_ordered([30_u64, 10, 20], 3, |delay| async move {
            sleep(Duration::from_millis(delay)).await;
            delay
        })
        .collect()
        .await;
        assert_eq!(outcomes, [30, 10, 20]);
    }

    #[tokio::test]
    async fn an_unordered_fan_out_yields_every_outcome_once() {
        let mut outcomes: Vec<u32> = fan_out_unordered(1..=5, 2, |item| ready(item * 10)).collect().await;
        outcomes.sort_unstable();
        assert_eq!(outcomes, [10, 20, 30, 40, 50]);
    }

    #[tokio::test]
    async fn no_more_than_the_limit_run_at_once() {
        let running = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let _: Vec<()> = fan_out_unordered(0..8, 3, |_| {
            let (running, peak) = (&running, &peak);
            async move {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                sleep(Duration::from_millis(5)).await;
                running.fetch_sub(1, Ordering::SeqCst);
            }
        })
        .collect()
        .await;
        assert_eq!(peak.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn the_items_up_to_the_limit_run_together() {
        let barrier = Barrier::new(3);
        let outcomes: Vec<usize> = fan_out_ordered(0..3, 3, |item| {
            let barrier = &barrier;
            async move {
                barrier.wait().await;
                item
            }
        })
        .collect()
        .await;
        assert_eq!(outcomes, [0, 1, 2]);
    }
}
