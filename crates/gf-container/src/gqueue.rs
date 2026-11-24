//! Port of `container/gqueue`.
//!
//! Go buffers through a channel of lists; the port uses a deque with two
//! condition variables. `push` blocks while a limited queue is full and `pop`
//! blocks while it is empty, exactly like the Go queue. [`Queue::close`]
//! wakes both sides; after closing, `pop` drains remaining values and then
//! returns `None`.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};

struct State<T> {
    values: VecDeque<T>,
    closed: bool,
}

/// A synchronized blocking queue.
pub struct Queue<T> {
    state: Mutex<State<T>>,
    not_empty: Condvar,
    not_full: Condvar,
    limit: Option<usize>,
}

impl<T> Queue<T> {
    /// Creates a queue, optionally limited in length.
    pub fn new(limit: Option<usize>) -> Queue<T> {
        Queue {
            state: Mutex::new(State {
                values: VecDeque::new(),
                closed: false,
            }),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
            limit: limit.filter(|limit| *limit > 0),
        }
    }

    /// Pushes a value, blocking when the queue is full.
    pub fn push(&self, value: T) {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(limit) = self.limit {
            while guard.values.len() >= limit && !guard.closed {
                guard = self
                    .not_full
                    .wait(guard)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }
        guard.values.push_back(value);
        self.not_empty.notify_one();
    }

    /// Pops a value, blocking when the queue is empty.
    ///
    /// Returns `None` once the queue is closed and drained.
    pub fn pop(&self) -> Option<T> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while guard.values.is_empty() && !guard.closed {
            guard = self
                .not_empty
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        let value = guard.values.pop_front();
        if value.is_some() {
            self.not_full.notify_one();
        }
        value
    }

    /// Closes the queue and wakes every waiter.
    pub fn close(&self) {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.closed = true;
        self.not_empty.notify_all();
        self.not_full.notify_all();
    }

    /// Returns the number of queued values.
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values
            .len()
    }

    /// Reports whether no value is queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Drop for Queue<T> {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn push_and_pop() {
        let queue: Queue<i64> = Queue::new(None);
        queue.push(1);
        queue.push(2);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        queue.close();
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn limit_blocks_producers_and_close_releases() {
        let queue: Arc<Queue<i64>> = Arc::new(Queue::new(Some(1)));
        queue.push(1);
        let producer_queue = Arc::clone(&queue);
        let producer = std::thread::spawn(move || {
            producer_queue.push(2);
        });
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.pop(), Some(1));
        producer.join().expect("producer");
        assert_eq!(queue.pop(), Some(2));
    }

    #[test]
    fn close_wakes_consumer() {
        let queue: Arc<Queue<i64>> = Arc::new(Queue::new(None));
        let consumer_queue = Arc::clone(&queue);
        let consumer = std::thread::spawn(move || consumer_queue.pop());
        std::thread::sleep(Duration::from_millis(20));
        queue.close();
        assert_eq!(consumer.join().expect("consumer"), None);
    }
}
