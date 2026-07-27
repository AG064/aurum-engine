//! Typed event bus.
//!
//! Events are values sent through the bus. Subscribers receive by type. The
//! bus is a queue — events emitted this tick are not delivered to subscribers
//! registered after the emit (or to subscribers registered during the emit
//! loop). After the tick, queued events are swapped into the "current" set
//! for delivery.
//!
//! This is the *core* bus. The GDScript shim exposes a fancier version that
//! also bridges to Godot signals.

use std::any::{Any, TypeId};
use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;

/// Trait for events. Anything `Send + Sync + 'static + Debug` is a valid event.
pub trait Event: Any + Send + Sync + Debug + 'static {}

impl<T> Event for T where T: Any + Send + Sync + Debug + 'static {}

type SubscriberId = u64;

pub struct EventBus {
    next_subscriber_id: SubscriberId,
    /// Subscribers keyed by event `TypeId`.
    subscribers: HashMap<TypeId, Vec<(SubscriberId, Box<dyn FnMut(&dyn Any)>)>>,
    /// Queue of events by type. Drained on `dispatch`.
    queue: VecDeque<(TypeId, Box<dyn Any>)>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            next_subscriber_id: 1,
            subscribers: HashMap::new(),
            queue: VecDeque::new(),
        }
    }

    /// Subscribe to events of type `E`. Returns an id that can be used to
    /// unsubscribe.
    pub fn subscribe<E, F>(&mut self, mut handler: F) -> SubscriberId
    where
        E: Event,
        F: FnMut(&E) + Send + Sync + 'static,
    {
        let id = self.next_subscriber_id;
        self.next_subscriber_id += 1;
        let boxed = move |raw: &dyn Any| {
            if let Some(event) = raw.downcast_ref::<E>() {
                handler(event);
            }
        };
        self.subscribers
            .entry(TypeId::of::<E>())
            .or_default()
            .push((id, Box::new(boxed)));
        id
    }

    /// Remove a subscriber by id. Returns whether it was found.
    pub fn unsubscribe(&mut self, id: SubscriberId) -> bool {
        for list in self.subscribers.values_mut() {
            if let Some(pos) = list.iter().position(|(sid, _)| *sid == id) {
                list.remove(pos);
                let _ = id;
                return true;
            }
        }
        false
    }

    /// Emit an event. It is queued and dispatched on the next `dispatch` call.
    pub fn emit<E: Event>(&mut self, event: E) {
        self.queue.push_back((TypeId::of::<E>(), Box::new(event)));
    }

    /// Drain the queue, delivering each event to its subscribers.
    ///
    /// Subscribers added during dispatch do NOT receive the current event
    /// (intentional — avoids reentrancy bugs).
    pub fn dispatch(&mut self) {
        // Move the queue out so subscribers can emit new events without
        // borrowing `self` recursively.
        let pending: Vec<_> = self.queue.drain(..).collect();
        for (type_id, event) in pending {
            if let Some(list) = self.subscribers.get_mut(&type_id) {
                for (_id, handler) in list.iter_mut() {
                    handler(event.as_ref());
                }
            }
        }
    }

    /// Number of pending events in the queue.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, PartialEq)]
    struct Hit {
        damage: i32,
    }

    #[derive(Debug)]
    struct Score {
        amount: i32,
    }

    #[test]
    fn deliver_to_subscribers() {
        let mut bus = EventBus::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        bus.subscribe::<Hit, _>(move |e| {
            log2.lock().unwrap().push(e.damage);
        });
        bus.emit(Hit { damage: 7 });
        bus.emit(Hit { damage: 13 });
        bus.dispatch();
        assert_eq!(*log.lock().unwrap(), vec![7, 13]);
    }

    #[test]
    fn unsubscribe_stops_delivery() {
        let mut bus = EventBus::new();
        let count = Arc::new(Mutex::new(0));
        let count2 = count.clone();
        let id = bus.subscribe::<Score, _>(move |_| {
            *count2.lock().unwrap() += 1;
        });
        bus.emit(Score { amount: 1 });
        bus.dispatch();
        assert_eq!(*count.lock().unwrap(), 1);
        bus.unsubscribe(id);
        bus.emit(Score { amount: 2 });
        bus.dispatch();
        assert_eq!(*count.lock().unwrap(), 1);
    }

    #[test]
    fn dispatch_drains_queue() {
        let mut bus = EventBus::new();
        bus.emit(Hit { damage: 1 });
        assert_eq!(bus.pending(), 1);
        bus.dispatch();
        assert_eq!(bus.pending(), 0);
    }
}
