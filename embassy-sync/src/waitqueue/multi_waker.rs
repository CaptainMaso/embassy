use core::{default, pin::Pin, task::Waker};

use pin_project::pin_project;

use crate::{
    blocking_mutex::raw::RawMutex,
    intrusive_list::{IntrusiveList, Node, NodeGuard},
};

/// Utility struct to register and wake multiple wakers.
pub struct MultiWakerRegistrar<M> {
    wakers: IntrusiveList<Waker, M>,
}

impl<M: RawMutex> MultiWakerRegistrar<M> {
    /// Create a new empty instance
    pub const fn new() -> Self {
        Self {
            wakers: IntrusiveList::new(),
        }
    }

    /// Register a waker.
    pub fn register<'s, 'p, 'r>(
        &'s self,
        mut store: Pin<&'p mut MultiWakerStorage>,
        waker: &Waker,
    ) -> MultiWakerRegistration<'r, M>
    where
        's: 'r,
        'p: 'r,
    {
        self.wakers.with_lock(move |lock| {
            if !self.wakers.any(|w| waker.will_wake(w), lock) {
                let _ = store.as_mut().node.insert(Node::new(waker.clone()));
                let store_ref = store.into_ref();
                let node_ref = store_ref.project_ref().node.as_pin_ref().unwrap();

                let guard = self.wakers.push_tail(node_ref, lock);
                MultiWakerRegistration {
                    inner: InnerReg::Registered { guard },
                }
            } else {
                MultiWakerRegistration {
                    inner: InnerReg::Unregistered { storage: store },
                }
            }
        })
    }

    pub fn update<'s, 'r>(&'s self, register: &mut MultiWakerRegistration<'r, M>, waker: &Waker)
    where
        's: 'r,
    {
        if !register.will_wake(waker) {
            match core::mem::take(&mut register.inner) {
                InnerReg::Empty => unreachable!(),
                InnerReg::Unregistered { storage } => {
                    let reg = self.register(storage, waker);
                    *register = reg;
                }
                InnerReg::Registered { guard } => {
                    guard.map(|w| w.clone_from(waker));
                    register.inner = InnerReg::Registered { guard };
                }
            }
        }
    }

    /// Wake all registered wakers. This clears the buffer
    pub fn wake(&self) {
        self.wakers
            .with_lock(|lock| self.wakers.for_each(|f| f.wake_by_ref(), lock));
    }
}

#[pin_project]
pub struct MultiWakerStorage {
    #[pin]
    node: Option<Node<Waker>>,
}

impl MultiWakerStorage {
    pub fn new() -> Self {
        Self { node: None }
    }
}

pub struct MultiWakerRegistration<'a, M: RawMutex> {
    inner: InnerReg<'a, M>,
}

impl<'a, M: RawMutex> MultiWakerRegistration<'a, M> {
    pub fn is_registered(&self) -> bool {
        matches!(self.inner, InnerReg::Registered { .. })
    }

    pub fn will_wake(&self, waker: &Waker) -> bool {
        match &self.inner {
            InnerReg::Empty => unreachable!(),
            InnerReg::Unregistered { storage } => false,
            InnerReg::Registered { guard } => guard.map(|w| w.will_wake(waker)),
        }
    }
}

impl<'a, M: RawMutex> Drop for MultiWakerRegistration<'a, M> {
    fn drop(&mut self) {
        if let InnerReg::Registered { guard } = &mut self.inner {
            guard.map(|w| w.wake_by_ref());
        }
    }
}

#[derive(Default)]
enum InnerReg<'a, M: RawMutex> {
    #[default]
    Empty,
    Unregistered {
        storage: Pin<&'a mut MultiWakerStorage>,
    },
    Registered {
        guard: NodeGuard<'a, Waker, M>,
    },
}
