//! Implementation of anything directly publisher related

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

use super::PubSubChannel;
use crate::blocking_mutex::raw::RawMutex;
use crate::waitqueue::{MultiWakerRegistration, MultiWakerStorage};

use pin_project::{pin_project, pinned_drop};

/// A publisher to a channel
#[pin_project(PinnedDrop)]
pub struct Pub<'a, M: RawMutex, T: Clone, const CAP: usize> {
    /// The channel we are a publisher for
    channel: &'a PubSubChannel<M, T, CAP>,
    #[pin]
    waker: MultiWakerStorage,
}

impl<'a, M: RawMutex, T: Clone, const CAP: usize> Pub<'a, M, T, CAP> {
    pub(super) fn new(channel: &'a PubSubChannel<M, T, CAP>) -> Self {
        Self {
            channel,
            waker: MultiWakerStorage::new(),
        }
    }

    /// Publish a message right now even when the queue is full.
    /// This may cause a subscriber to miss an older message.
    pub fn publish_immediate(&self, message: T) {
        self.channel.publish_immediate(message)
    }

    /// Publish a message. But if the message queue is full, wait for all subscribers to have read the last message
    pub fn publish<'s, 'r>(&'s mut self, message: T) -> PublisherWaitFuture<'s, 'a, M, T, CAP> {
        PublisherWaitFuture::new(self, message)
    }

    /// Publish a message if there is space in the message queue
    pub fn try_publish(&self, message: T) -> Result<(), T> {
        self.channel.try_publish(message)
    }

    /// The amount of messages that can still be published without having to wait or without having to lag the subscribers
    ///
    /// *Note: In the time between checking this and a publish action, other publishers may have had time to publish something.
    /// So checking doesn't give any guarantees.*
    pub fn space(&self) -> usize {
        self.channel.space()
    }
}

#[pinned_drop]
impl<'a, M: RawMutex, T: Clone, const CAP: usize> PinnedDrop for Pub<'a, M, T, CAP> {
    fn drop(self: Pin<&mut Self>) {
        self.channel.unregister_publisher();
    }
}

/// Future for the publisher wait action
#[repr(transparent)]
pub struct PublisherWaitFuture<'s, 'a, M: RawMutex, T: Clone, const CAP: usize>(
    InnerPublisherWaitFuture<'s, 'a, M, T, CAP>,
);

impl<'s, 'a, M: RawMutex, T: Clone, const CAP: usize> PublisherWaitFuture<'s, 'a, M, T, CAP> {
    /// Creates a new `PublisherWaitFuture`
    pub fn new(publisher: &'s mut Pub<'a, M, T, CAP>, message: T) -> Self {
        Self(InnerPublisherWaitFuture::Init {
            message,
            publisher: core::pin::Pin::new(publisher),
        })
    }
}

#[derive(Default)]
#[pin_project]
#[must_use = "futures do nothing unless you `.await` or poll them"]
enum InnerPublisherWaitFuture<'s, 'a, M: RawMutex, T: Clone, const CAP: usize> {
    /// The message we need to publish
    Init {
        message: T,
        publisher: Pin<&'s mut Pub<'a, M, T, CAP>>,
    },
    Registered {
        message: T,
        ch: &'a PubSubChannel<M, T, CAP>,
        reg: MultiWakerRegistration<'s, M>,
    },
    #[default]
    Complete,
}

impl<'s, 'a, M: RawMutex, T: Clone, const CAP: usize> Future for PublisherWaitFuture<'s, 'a, M, T, CAP> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let s = self.get_mut();
        match core::mem::take(&mut s.0) {
            InnerPublisherWaitFuture::Init { message, publisher } => {
                let p = publisher.project();
                let ch = *p.channel;
                let store = p.waker;

                let Err(message) = ch.try_publish(message) else {
                    return Poll::Ready(());
                };

                let reg = ch.publisher_wakers.register(store, cx.waker());
                s.0 = InnerPublisherWaitFuture::Registered { message, ch, reg };
            }
            InnerPublisherWaitFuture::Registered { message, ch, mut reg } => {
                let Err(message) = ch.try_publish(message) else {
                    return Poll::Ready(());
                };
                ch.publisher_wakers.update(&mut reg, cx.waker());
                s.0 = InnerPublisherWaitFuture::Registered { message, ch, reg };
            }
            InnerPublisherWaitFuture::Complete => unreachable!(),
        }
        Poll::Pending
    }
}
