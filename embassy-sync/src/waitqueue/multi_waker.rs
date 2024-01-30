use core::default;
use core::pin::Pin;
use core::task::Waker;

use pin_project::pin_project;

use crate::blocking_mutex::raw::{ConstRawMutex, RawMutex};
use crate::intrusive_list::{IntrusiveList, Item};

/// Utility struct to register and wake multiple wakers.
pub struct MultiWakerRegistrar<M: RawMutex> {
    wakers: IntrusiveList<Waker, M>,
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

    // /// Register a waker.
    // pub fn register(&'s self) -> Self::M
    // where
    //     's: 'r,
    //     'p: 'r,
    // {
    //     self.wakers.with_lock(move |lock| {
    //         if !self.wakers.any(|w| waker.will_wake(w), lock) {
    //             let _ = store.as_mut().node.insert(Node::new(waker.clone()));
    //             let store_ref = store.into_ref();
    //             let node_ref = store_ref.project_ref().node.as_pin_ref().unwrap();

    //             let guard = self.wakers.push_tail(node_ref, lock);
    //             MultiWakerRegistration {
    //                 inner: InnerReg::Registered { guard },
    //             }
    //         } else {
    //             MultiWakerRegistration {
    //                 inner: InnerReg::Unregistered { storage: store },
    //             }
    //         }
    //     })
    // }

    // pub fn update<'s, 'r>(&'s self, register: &mut MultiWakerRegistration<'r, M>, waker: &Waker)
    // where
    //     's: 'r,
    // {
    //     if !register.will_wake(waker) {
    //         match core::mem::take(&mut register.inner) {
    //             InnerReg::Empty => unreachable!(),
    //             InnerReg::Unregistered { storage } => {
    //                 let reg = self.register(storage, waker);
    //                 *register = reg;
    //             }
    //             InnerReg::Registered { guard } => {
    //                 guard.map(|w| w.clone_from(waker));
    //                 register.inner = InnerReg::Registered { guard };
    //             }
    //         }
    //     }
    // }

    // /// Wake all registered wakers. This clears the buffer
    // pub fn wake(&self) {
    //     self.wakers
    //         .with_lock(|lock| self.wakers.for_each(|f| f.wake_by_ref(), lock));
    // }
}

#[pin_project]
pub struct MultiWakerStorage<'a, M: RawMutex> {
    #[pin]
    node: Item<'a, Option<Waker>, M>,
}
