//! Implementation of [PubSubChannel], a queue where published messages get received by all subscribers.

#![deny(missing_docs)]

use core::cell::RefCell;
use core::fmt::Debug;

use crate::deque::{Deque, DequeRef};

use self::publisher::Pub;
use self::subscriber::Sub;
use crate::blocking_mutex::raw::RawMutex;
use crate::waitqueue::MultiWakerRegistrar;

pub mod publisher;
pub mod subscriber;
#[cfg(test)]
mod test;


/// A broadcast channel implementation where multiple publishers can send messages to multiple subscribers
///
/// Any published message can be read by all subscribers.
/// A publisher can choose how it sends its message.
///
/// - With [Pub::publish()] the publisher has to wait until there is space in the internal message queue.
/// - With [Pub::publish_immediate()] the publisher doesn't await and instead lets the oldest message
/// in the queue drop if necessary. This will cause any [Subscriber] that missed the message to receive
/// an error to indicate that it has lagged.
///
/// ## Example
///
/// ```
/// # use embassy_sync::blocking_mutex::raw::NoopRawMutex;
/// # use embassy_sync::pubsub::WaitResult;
/// # use embassy_sync::pubsub::PubSubChannel;
/// # use futures_executor::block_on;
/// # let test = async {
/// // Create the channel. This can be static as well
/// let channel = PubSubChannel::<NoopRawMutex, u32, 4, 4, 4>::new();
///
/// // This is a generic subscriber with a direct reference to the channel
/// let mut sub0 = channel.subscriber().unwrap();
/// // This is a dynamic subscriber with a dynamic (trait object) reference to the channel
/// let mut sub1 = channel.dyn_subscriber().unwrap();
///
/// let pub0 = channel.publisher().unwrap();
///
/// // Publish a message, but wait if the queue is full
/// pub0.publish(42).await;
///
/// // Publish a message, but if the queue is full, just kick out the oldest message.
/// // This may cause some subscribers to miss a message
/// pub0.publish_immediate(43);
///
/// // Wait for a new message. If the subscriber missed a message, the WaitResult will be a Lag result
/// assert_eq!(sub0.next_message().await, WaitResult::Message(42));
/// assert_eq!(sub1.next_message().await, WaitResult::Message(42));
///
/// // Wait again, but this time ignore any Lag results
/// assert_eq!(sub0.next_message_pure().await, 43);
/// assert_eq!(sub1.next_message_pure().await, 43);
///
/// // There's also a polling interface
/// assert_eq!(sub0.try_next_message(), None);
/// assert_eq!(sub1.try_next_message(), None);
/// # };
/// #
/// # block_on(test);
/// ```
///
pub struct PubSubChannel<M: RawMutex, T: Clone, const CAP: usize> {
    mutex: M,
    state: RefCell<PubSubState<T, CAP>>,
    /// Collection of wakers for Subscribers that are waiting.  
    subscriber_wakers: MultiWakerRegistrar<M>,
    /// Collection of wakers for Publishers that are waiting.  
    publisher_wakers: MultiWakerRegistrar<M>,
}

impl<M: RawMutex, T: Clone, const CAP: usize> PubSubChannel<M, T, CAP> {
    /// Create a new channel
    pub const fn new() -> Self {
        Self {
            mutex: M::INIT,
            state: RefCell::new(PubSubState::new()),
            subscriber_wakers: MultiWakerRegistrar::new(),
            publisher_wakers: MultiWakerRegistrar::new(),
        }
    }

    /// Create a new subscriber. It will only receive messages that are published after its creation.
    pub fn subscriber(&self) -> Sub<M, T, CAP> {
        let next_id = self.mutex.lock(|| {
            let mut s = self.state.borrow_mut();

            s.subscriber_count += 1;
            s.next_message_id
        });
        Sub::new(next_id, self)
    }

    /// Create a new publisher
    pub fn publisher(&self) -> Pub<M, T, CAP> {
        self.mutex.lock(|| {
            let mut s = self.state.borrow_mut();
            s.publisher_count += 1;
        });
        Pub::new(self)
    }

    /// Tries to publish a message to the queue
    ///
    /// # Safety
    ///
    /// Assumes that the mutex has been locked
    unsafe fn try_publish_unchecked(&self, message: T) -> Result<(), T> {
        let mut l = self.state.borrow_mut();
        if l.subscriber_count == 0 {
            // We don't need to publish anything because there is no one to receive it
            return Ok(());
        }

        if l.queue.is_full() {
            return Err(message);
        }
        // We just did a check for this
        let sub_count = l.subscriber_count;
        l.queue.push_back((message, sub_count)).ok().unwrap();

        l.next_message_id += 1;

        // Wake all of the subscribers
        self.subscriber_wakers.wake();

        Ok(())
    }

    fn try_publish(&self, message: T) -> Result<(), T> {
        self.mutex.lock(|| 
            // Safety: This is safe because we have locked the mutex
            unsafe { self.try_publish_unchecked(message) }
        )
    }

    fn publish_immediate(&self, message: T) {
        self.mutex.lock(|| {
            {
                let mut l = self.state.borrow_mut();
                // Make space in the queue if required
                if l.queue.is_full() {
                    l.queue.pop_front();
                }
                core::mem::drop(l);
            }

            // This will succeed because we made sure there is space
            // Safety: This is safe because we have already locked the mutex
            unsafe { self.try_publish_unchecked(message) }.ok().unwrap();
        });
    }

    fn get_message(&self, message_id: u64) -> Option<WaitResult<T>> {
        self.mutex.lock(|| {
            let mut l = self.state.borrow_mut();
            let start_id = l.next_message_id - l.queue.len() as u64;

            if message_id < start_id {
                return Some(WaitResult::Lagged(start_id - message_id));
            }

            let current_message_index = (message_id - start_id) as usize;

            if current_message_index >= l.queue.len() {
                return None;
            }

            // We've checked that the index is valid
            let queue_item = l.queue.iter_mut().nth(current_message_index).unwrap();

            // We're reading this item, so decrement the counter
            queue_item.1 -= 1;

            let message = if current_message_index == 0 && queue_item.1 == 0 {
                let (message, _) = l.queue.pop_front().unwrap();
                self.publisher_wakers.wake();
                // Return pop'd message without clone
                message
            } else {
                queue_item.0.clone()
            };

            Some(WaitResult::Message(message))
        })
    }

    fn unregister_subscriber(&self, subscriber_next_message_id: u64) {
        self.mutex.lock(|| {
            let mut l = self.state.borrow_mut();
            l.subscriber_count -= 1;

            // All messages that haven't been read yet by this subscriber must have their counter decremented
            let start_id = l.next_message_id - l.queue.len() as u64;
            if subscriber_next_message_id >= start_id {
                let current_message_index = (subscriber_next_message_id - start_id) as usize;
                l.queue
                    .iter_mut()
                    .skip(current_message_index)
                    .for_each(|(_, counter)| *counter -= 1);

                let mut wake_publishers = false;
                while let Some((_, count)) = l.queue.front() {
                    if *count == 0 {
                        l.queue.pop_front().unwrap();
                        wake_publishers = true;
                    } else {
                        break;
                    }
                }

                if wake_publishers {
                    self.publisher_wakers.wake();
                }
            }
        })
    }

    fn unregister_publisher(&self) {
        self.mutex.lock(|| {
            let mut l = self.state.borrow_mut();
            l.publisher_count -= 1;
        })
    }

    fn space(&self) -> usize {
        self.mutex.lock(|| {
            let s = self.state.borrow();
            s.queue.capacity() - s.queue.len()
        })
    }

    fn available(&self, next_message_id: u64) -> u64 {
        self.mutex.lock(|| {
            let s = self.state.borrow();
            s.next_message_id - next_message_id
        })
    }
}

/// Internal state for the PubSub channel
#[repr(C)]
struct PubSubState<T: Clone, const CAP: usize> {
    /// Every message has an id.
    /// Don't worry, we won't run out.
    /// If a million messages were published every second, then the ID's would run out in about 584942 years.
    next_message_id: u64,
    /// The amount of subscribers that are active
    subscriber_count: usize,
    /// The amount of publishers that are active
    publisher_count: usize,
    /// The queue contains the last messages that have been published and a countdown of how many subscribers are yet to read it
    queue: Deque<(T, usize), CAP>,
}

#[repr(C)]
struct PubSubStateRef<T : Clone> {
    /// Every message has an id.
    /// Don't worry, we won't run out.
    /// If a million messages were published every second, then the ID's would run out in about 584942 years.
    next_message_id: u64,
    /// The amount of subscribers that are active
    subscriber_count: usize,
    /// The amount of publishers that are active
    publisher_count: usize,
    /// The queue contains the last messages that have been published and a countdown of how many subscribers are yet to read it
    queue: DequeRef<(T, usize)>,
}

impl<T: Clone, const CAP: usize> PubSubState<T, CAP> {
    /// Create a new internal channel state
    const fn new() -> Self {
        Self {
            queue: Deque::new(),
            next_message_id: 0,
            subscriber_count: 0,
            publisher_count: 0,
        }
    }
}

/// Error type for the [PubSubChannel]
#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// All subscriber slots are used. To add another subscriber, first another subscriber must be dropped or
    /// the capacity of the channels must be increased.
    MaximumSubscribersReached,
    /// All publisher slots are used. To add another publisher, first another publisher must be dropped or
    /// the capacity of the channels must be increased.
    MaximumPublishersReached,
}

/// The result of the subscriber wait procedure
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum WaitResult<T> {
    /// The subscriber did not receive all messages and lagged by the given amount of messages.
    /// (This is the amount of messages that were missed)
    Lagged(u64),
    /// A message was received
    Message(T),
}

impl<T> WaitResult<T> {
    fn msg_id_incr(&self) -> u64 {
        match self {
            WaitResult::Lagged(l) => *l,
            WaitResult::Message(_) => 1,
        }
    }
}
