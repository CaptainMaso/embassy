use core::pin::Pin;
use core::task::Waker;

use pin_project::pin_project;

use crate::blocking_mutex::raw::{ConstRawMutex, CriticalSectionRawMutex, RawMutex};
use crate::intrusive_list::{IntrusiveList, Item};

/// Utility struct to register and wake multiple wakers.
pub struct MultiWaker<M: RawMutex> {
    wakers: IntrusiveList<Option<Waker>, M>,
}

impl<M: RawMutex> MultiWaker<M> {
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
    pub fn store(&self) -> MultiWakerStore<'_, M> {
        MultiWakerStore {
            node: self.wakers.new_store(None),
        }
    }

    pub fn update(&self, store: Pin<&mut MultiWakerStore<'_, M>>, waker: &Waker) {
        let n = store.project().node;

        if n.as_ref().is_linked() {
            n.lock(|c| {
                if let Some(c) = c.as_mut() {
                    if !c.will_wake(waker) {
                        *c = waker.clone();
                    }
                } else {
                    let _ = c.insert(waker.clone());
                }
            });
        } else {
            self.wakers.with_cursor(|cursor| {
                let p = cursor.position(|_, n| if let Some(n) = n { n.will_wake(waker) } else { false });
                if p.is_none() {
                    let mut n = cursor.insert_tail(n.as_ref());
                    let _ = n.insert(waker.clone());
                }
            })
        }
    }

    /// Wake all registered wakers.
    pub fn wake(&self) {
        self.wakers.with_cursor(|cursor| {
            cursor.retain(|_, w| {
                if let Some(w) = w {
                    w.wake_by_ref();
                    true
                } else {
                    false
                }
            });
        });
    }
}

#[pin_project]
pub struct MultiWakerStore<'a, M: RawMutex> {
    #[pin]
    node: Item<'a, Option<Waker>, M>,
}

#[allow(dead_code)]
mod test {
    use super::*;

    const fn is_send<T: Send>() {}
    const fn is_sync<T: Sync>() {}

    const REGISTRATION_SEND: () = is_send::<MultiWakerStore<CriticalSectionRawMutex>>();
    const REGISTRATION_SYNC: () = is_sync::<MultiWakerStore<CriticalSectionRawMutex>>();
    const REGISTRAR_SEND: () = is_send::<MultiWaker<CriticalSectionRawMutex>>();
    const REGISTRAR_SYNC: () = is_sync::<MultiWaker<CriticalSectionRawMutex>>();
}
