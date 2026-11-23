use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::types::Bar;

/// Min-heap of market events.
///
/// The ordering key is `(timestamp, symbol)`. Insertion order is deliberately
/// absent from the key, so feeding the same rows in a different order cannot
/// change the simulation.
#[derive(Clone, Debug, Default)]
pub struct EventQueue {
    heap: BinaryHeap<std::cmp::Reverse<QueuedEvent>>,
}

#[derive(Clone, Debug)]
struct QueuedEvent {
    ts: i64,
    symbol: String,
    bar: Bar,
}

impl PartialEq for QueuedEvent {
    fn eq(&self, other: &Self) -> bool {
        self.ts == other.ts && self.symbol == other.symbol
    }
}

impl Eq for QueuedEvent {}

impl PartialOrd for QueuedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueuedEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.ts, &self.symbol).cmp(&(other.ts, &other.symbol))
    }
}

impl EventQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bar: Bar) {
        self.heap.push(std::cmp::Reverse(QueuedEvent {
            ts: bar.ts,
            symbol: bar.symbol.clone(),
            bar,
        }));
    }

    pub fn pop(&mut self) -> Option<Bar> {
        self.heap.pop().map(|event| event.0.bar)
    }

    pub fn peek_ts(&self) -> Option<i64> {
        self.heap.peek().map(|event| event.0.ts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(symbol: &str, ts: i64) -> Bar {
        Bar {
            symbol: symbol.into(),
            ts,
            published_at: ts,
            version: 1,
            open: 10.0,
            high: 11.0,
            low: 9.0,
            close: 10.5,
            volume: 1_000.0,
        }
    }

    #[test]
    fn pops_by_timestamp_then_symbol() {
        let mut queue = EventQueue::new();
        for (symbol, ts) in [
            ("BBB", 20),
            ("AAA", 30),
            ("CCC", 10),
            ("AAA", 20),
            ("BBB", 30),
        ] {
            queue.push(bar(symbol, ts));
        }
        let mut order = Vec::new();
        while let Some(bar) = queue.pop() {
            order.push((bar.ts, bar.symbol));
        }
        assert_eq!(
            order,
            vec![
                (10, "CCC".to_string()),
                (20, "AAA".to_string()),
                (20, "BBB".to_string()),
                (30, "AAA".to_string()),
                (30, "BBB".to_string()),
            ]
        );
    }

    #[test]
    fn insertion_order_does_not_matter() {
        let bars = vec![bar("BBB", 20), bar("AAA", 10), bar("CCC", 20)];
        let mut forward = EventQueue::new();
        for bar in bars.clone() {
            forward.push(bar);
        }
        let mut backward = EventQueue::new();
        for bar in bars.into_iter().rev() {
            backward.push(bar);
        }
        let mut left = Vec::new();
        while let Some(bar) = forward.pop() {
            left.push((bar.ts, bar.symbol));
        }
        let mut right = Vec::new();
        while let Some(bar) = backward.pop() {
            right.push((bar.ts, bar.symbol));
        }
        assert_eq!(left, right);
    }
}
