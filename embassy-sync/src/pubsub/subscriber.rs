//! Implementation of anything directly subscriber related

use core::future::Future;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;
use core::task::{Context, Poll};

use pin_project::{pin_project, pinned_drop};

use super::{PubSubChannel, WaitResult};
use crate::blocking_mutex::raw::RawMutex;
use crate::waitqueue::{MultiWakerRegistration, MultiWakerStorage};

/// A subscriber to a channel
#[pin_project(PinnedDrop)]
pub struct Sub<'a, M: RawMutex, T: Clone, const CAP: usize> {
    /// The message id of the next message we are yet to receive
    next_message_id: u64,
    /// The channel we are a subscriber to
    channel: &'a PubSubChannel<M, T, CAP>,
    #[pin]
    waker: MultiWakerStorage,
}

impl<'a, M: RawMutex, T: Clone, const CAP: usize> Sub<'a, M, T, CAP> {
    pub(super) fn new(next_message_id: u64, channel: &'a PubSubChannel<M, T, CAP>) -> Self {
        Self {
            next_message_id,
            channel,
            waker: MultiWakerStorage::new(),
        }
    }

    /// Wait for a published message
    pub fn next_message<'s>(&'s mut self) -> SubscriberWaitFuture<'s, 'a, M, T, CAP> {
        SubscriberWaitFuture::new(self)
    }

    /// Wait for a published message (ignoring lag results)
    pub async fn next_message_pure(&mut self) -> T {
        let mut s = core::pin::Pin::new(self);
        loop {
            match s.as_mut().next_message().await {
                WaitResult::Lagged(_) => continue,
                WaitResult::Message(message) => break message,
            }
        }
    }

    /// Try to see if there's a published message we haven't received yet.
    ///
    /// This function does not peek. The message is received if there is one.
    pub fn try_next_message(&mut self) -> Option<WaitResult<T>> {
        let res = self.channel.get_message(self.next_message_id);

        match &res {
            Some(WaitResult::Lagged(lagged)) => {
                self.next_message_id += *lagged;
            }
            Some(WaitResult::Message(_)) => {
                self.next_message_id += 1;
            }
            None => (),
        }

        res
    }

    /// Try to see if there's a published message we haven't received yet (ignoring lag results).
    ///
    /// This function does not peek. The message is received if there is one.
    pub fn try_next_message_pure(&mut self) -> Option<T> {
        loop {
            match self.try_next_message() {
                Some(WaitResult::Lagged(_)) => continue,
                Some(WaitResult::Message(message)) => break Some(message),
                None => break None,
            }
        }
    }

    /// The amount of messages this subscriber hasn't received yet
    pub fn available(&self) -> u64 {
        self.channel.available(self.next_message_id)
    }
}

#[pinned_drop]
impl<'a, M: RawMutex, T: Clone, const CAP: usize> PinnedDrop for Sub<'a, M, T, CAP> {
    fn drop(self: Pin<&mut Self>) {
        self.channel.unregister_subscriber(self.next_message_id);
    }
}

// #[pin_project]
// pub struct SubStream<'s, 'a, M: RawMutex, T: Clone, const CAP: usize> {
//     inner: InnerSubscriberWaitFuture<'s, 'a, M, T, CAP>,
// }

// // /// Warning: The stream implementation ignores lag results and returns all messages.
// // /// This might miss some messages without you knowing it.
// impl<'a, M: RawMutex, T: Clone, const CAP: usize> futures_util::Stream for Sub<'a, M, T, CAP> {
//     type Item = T;

//     fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         match self
//             .channel
//             .get_message_with_context(&mut self.next_message_id, Some(cx))
//         {
//             Poll::Ready(WaitResult::Message(message)) => Poll::Ready(Some(message)),
//             Poll::Ready(WaitResult::Lagged(_)) => {
//                 cx.waker().wake_by_ref();
//                 Poll::Pending
//             }
//             Poll::Pending => Poll::Pending,
//         }
//     }
// }

/// Future for the Subscriber wait action
#[repr(transparent)]
pub struct SubscriberWaitFuture<'s, 'a, M: RawMutex, T: Clone, const CAP: usize>(
    InnerSubscriberWaitFuture<'s, 'a, M, T, CAP>,
);

impl<'s, 'a, M: RawMutex, T: Clone, const CAP: usize> SubscriberWaitFuture<'s, 'a, M, T, CAP> {
    /// Creates a new `SubscriberWaitFuture`
    pub fn new(subscriber: &'s mut Sub<'a, M, T, CAP>) -> Self {
        Self(InnerSubscriberWaitFuture::Init {
            subscriber: core::pin::Pin::new(subscriber),
        })
    }
}

#[derive(Default)]
#[pin_project]
#[must_use = "futures do nothing unless you `.await` or poll them"]
enum InnerSubscriberWaitFuture<'s, 'a, M: RawMutex, T: Clone, const CAP: usize> {
    /// The message we need to publish
    Init {
        subscriber: Pin<&'s mut Sub<'a, M, T, CAP>>,
    },
    Registered {
        ch: &'a PubSubChannel<M, T, CAP>,
        msg_id: &'s mut u64,
        reg: MultiWakerRegistration<'s, M>,
    },
    #[default]
    Complete,
}

impl<'s, 'a, M: RawMutex, T: Clone, const CAP: usize> Future for SubscriberWaitFuture<'s, 'a, M, T, CAP> {
    type Output = WaitResult<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let s = self.get_mut();
        match core::mem::take(&mut s.0) {
            InnerSubscriberWaitFuture::Init { subscriber } => {
                let p = subscriber.project();
                let ch = *p.channel;
                let store = p.waker;
                let msg_id = p.next_message_id;

                if let Some(r) = ch.get_message(*msg_id) {
                    *msg_id += r.msg_id_incr();
                    return Poll::Ready(r);
                }

                let reg = ch.subscriber_wakers.register(store, cx.waker());
                s.0 = InnerSubscriberWaitFuture::Registered { msg_id, ch, reg };
            }
            InnerSubscriberWaitFuture::Registered { msg_id, ch, mut reg } => {
                if let Some(r) = ch.get_message(*msg_id) {
                    *msg_id += r.msg_id_incr();
                    return Poll::Ready(r);
                }
                ch.subscriber_wakers.update(&mut reg, cx.waker());
                s.0 = InnerSubscriberWaitFuture::Registered { msg_id, ch, reg };
            }
            InnerSubscriberWaitFuture::Complete => unreachable!(),
        }
        Poll::Pending
    }
}
