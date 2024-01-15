use core::task::Waker;

use crate::{
    blocking_mutex::raw::RawMutex,
    intrusive_list::{IntrusiveList, Node, NodeRef},
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

    fn proj_wakers(self: core::pin::Pin<&Self>) -> core::pin::Pin<&IntrusiveList<Waker, M>> {}

    /// Register a waker. If the buffer is full the function returns it in the error
    pub fn register<'a>(self: core::pin::Pin<&'a Self>, w: &'a MultiWakerStorage<'_>) {
        let l = self.proj_wakers();
        l.with_lock(|lock| if l.any(|o| w.node.will_wake(o), lock) {})
    }

    /// Wake all registered wakers. This clears the buffer
    pub fn wake(&mut self) {
        // heapless::Vec has no `drain()`, do it unsafely ourselves...

        // First set length to 0, without dropping the contents.
        // This is necessary for soundness: if wake() panics and we're using panic=unwind.
        // Setting len=0 upfront ensures other code can't observe the vec in an inconsistent state.
        // (it'll leak wakers, but that's not UB)
        // let len = self.wakers.len();
        // unsafe { self.wakers.set_len(0) }

        // for i in 0..len {
        //     // Move a waker out of the vec.
        //     let waker = unsafe { self.wakers.as_mut_ptr().add(i).read() };
        //     // Wake it by value, which consumes (drops) it.
        //     waker.wake();
        // }
        todo!()
    }
}

pub struct MultiWakerStorage<'a> {
    node: Node<&'a Waker>,
}

impl<'a> MultiWakerStorage<'a> {
    pub fn new(waker: &'a Waker) -> Self {
        Self { node: Node::new(waker) }
    }
}
