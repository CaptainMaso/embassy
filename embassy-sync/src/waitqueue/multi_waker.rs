use core::default;
use core::pin::Pin;
use core::task::Waker;

use pin_project::pin_project;

use crate::blocking_mutex::raw::{ConstRawMutex, CriticalSectionRawMutex, RawMutex};
use crate::intrusive_list::{IntrusiveList, Item};

/// Utility struct to register and wake multiple wakers.
pub struct MultiWakerRegistrar<M: RawMutex> {
    wakers: IntrusiveList<Option<Waker>, M>,
}

impl<M: RawMutex> MultiWakerRegistrar<M> {
    /// Create a new empty instance
    pub const fn new() -> Self
    where
        M: ConstRawMutex,
    {
        Self {
            wakers: IntrusiveList::new(),
        }
    }

    /// Register a waker.
    pub fn new_registration(&self) -> MultiWakerRegistration<'_, M> {
        MultiWakerRegistration {
            node: self.wakers.new_store(None),
        }
    }

    // /// Wake all registered wakers. This clears the buffer
    // pub fn wake(&self) {
    //     self.wakers
    //         .with_lock(|lock| self.wakers.for_each(|f| f.wake_by_ref(), lock));
    // }
}

#[pin_project]
pub struct MultiWakerRegistration<'a, M: RawMutex> {
    #[pin]
    node: Item<'a, Option<Waker>, M>,
}

impl<'a, M: RawMutex> MultiWakerRegistration<'a, M> {
    pub fn update(self: Pin<&Self>, waker: &Waker) {
        let n = self.project_ref().node;

        n.with_cursor(|c| {
            
        });

        if  {
            n.lock(|s| {
                let s = s.get_or_insert_with(|| waker.clone());
                if !s.will_wake(waker) {
                    *s = waker.clone();
                }
            });
        }
    }
}

const fn is_send<T: Send>() {}
const fn is_sync<T: Sync>() {}

const REGISTRAR_SEND: () = is_send::<MultiWakerRegistration<CriticalSectionRawMutex>>();
const REGISTRAR_SYNC: () = is_sync::<MultiWakerRegistration<CriticalSectionRawMutex>>();
const REGISTRATION_SEND: () = is_send::<MultiWakerRegistrar<CriticalSectionRawMutex>>();
const REGISTRATION_SYNC: () = is_sync::<MultiWakerRegistrar<CriticalSectionRawMutex>>();
