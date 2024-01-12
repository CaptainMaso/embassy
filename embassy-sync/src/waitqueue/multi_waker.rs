use core::task::Waker;

use crate::{
    blocking_mutex::raw::RawMutex,
    intrusive_list::{NodeRef, RawIntrusiveList},
};

/// Utility struct to register and wake multiple wakers.
pub struct MultiWakerRegistrar {
    wakers: RawIntrusiveList<Waker>,
}

impl MultiWakerRegistrar {
    /// Create a new empty instance
    pub const fn new() -> Self {
        Self {
            wakers: RawIntrusiveList::new(),
        }
    }

    /// Register a waker. If the buffer is full the function returns it in the error
    pub fn register<'a>(&self, w: &'a Waker) {
        // // If we already have some waker that wakes the same task as `w`, do nothing.
        // // This avoids cloning wakers, and avoids unnecessary mass-wakes.
        // for w2 in &self.wakers {
        //     if w.will_wake(w2) {
        //         return;
        //     }
        // }

        // if self.wakers.is_full() {
        //     // All waker slots were full. It's a bit inefficient, but we can wake everything.
        //     // Any future that is still active will simply reregister.
        //     // This won't happen a lot, so it's ok.
        //     self.wake();
        // }

        // if self.wakers.push(w.clone()).is_err() {
        //     // This can't happen unless N=0
        //     // (Either `wakers` wasn't full, or it was in which case `wake()` empied it)
        //     panic!("tried to push a waker to a zero-length MultiWakerRegistration")
        // }
        todo!()
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

pub struct MultiWakerStorage {}
