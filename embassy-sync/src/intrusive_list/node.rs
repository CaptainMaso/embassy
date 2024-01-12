use core::pin::Pin;

use crate::{blocking_mutex::raw::RawMutex, debug_cell::DebugCell};

use super::{raw::ListLock, RawIntrusiveList};

pub struct Node<T> {
    data: DebugCell<T>,
    //prev: UnsafeCell<Option<NonNull<Node<T>>>>,
    next: DebugCell<Option<NodeRef<T>>>,
}

impl<T> Node<T> {
    pub fn new(data: T) -> Self {
        Self {
            data: DebugCell::new(data),
            //prev: UnsafeCell::new(None),
            next: DebugCell::new(None),
        }
    }

    /// Gets a shared reference to the node data
    ///
    /// SAFETY: Assumes that the caller has a valid shared reference to the `RawIntrusiveList`
    unsafe fn get_data(&self) -> impl core::ops::Deref<Target = T> + '_ {
        self.data.borrow()
    }

    /// Gets a mutable reference to the node data
    ///
    /// SAFETY: Assumes that the caller has a valid unique reference to the `RawIntrusiveList`
    unsafe fn get_data_mut(&self) -> impl core::ops::DerefMut<Target = T> + '_ {
        self.data.borrow_mut()
    }

    // /// Reads the value of the prev node
    // ///
    // /// SAFETY: Assumes that the caller has locked the list mutex
    // unsafe fn get_prev_assume_locked(&self) -> Option<NonNull<Node<T>>> {
    //     unsafe { self.prev.get().read() }
    // }

    /// Gets a shared reference to the next node reference
    ///
    /// SAFETY: Assumes that the caller has a valid shared reference to
    /// the `RawIntrusiveList`.
    unsafe fn get_next(&self) -> impl core::ops::Deref<Target = Option<NodeRef<T>>> + '_ {
        self.next.borrow()
    }

    /// Gets a mutable the value of the next node
    ///
    /// SAFETY: Assumes that the caller has a valid unique reference to the `RawIntrusiveList`
    unsafe fn get_next_mut(&self) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + '_ {
        self.next.borrow_mut()
    }
}

#[derive(Debug)]
pub struct NodeRef<T> {
    #[cfg(debug_assertions)]
    list_ptr: *const (),
    ptr: *const Node<T>,
}

impl<T> Clone for NodeRef<T> {
    fn clone(&self) -> Self {
        Self {
            list_ptr: self.list_ptr,
            ptr: self.ptr,
        }
    }
}

impl<T> Copy for NodeRef<T> {}

impl<T> PartialEq for NodeRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.list_ptr == other.list_ptr && self.ptr == other.ptr
    }
}

impl<T> Eq for NodeRef<T> {}

impl<T> NodeRef<T> {
    /// Gets a reference to the `Node<T>`
    ///
    /// SAFETY: The caller must ensure that there are no
    /// other mutable references to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_node_unchecked(&self) -> &Node<T> {
        self.ptr.as_ref().unwrap()
    }

    /// Gets a reference to the `Node<T>`
    ///
    /// The supplied `ListLock` must be created from the
    /// registered `(Raw)IntrusiveList`, otherwise it will
    /// panic (only in debug mode).
    #[inline(always)]
    pub fn get_node<'l>(&'l self, lock: &'l ListLock) -> &'l Node<T> {
        lock.
        unsafe { self.get_node_unchecked() }
    }

    /// Gets a reference to the data stored in the node `T`
    ///
    /// SAFETY: The caller must ensure that there are no
    /// other mutable references to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_data_unchecked<'l>(&'l self) -> impl core::ops::Deref<Target = T> + 'l {
        let n = self.get_node_unchecked();
        n.get_data()
    }

    /// Gets a reference to the data stored in the node `T`
    ///
    /// The supplied `ListLock` must be created from the
    /// registered `(Raw)IntrusiveList`, otherwise it will
    /// panic (only in debug mode).
    #[inline(always)]
    pub fn get_data<'l>(&'l self, lock: &'l ListLock) -> impl core::ops::Deref<Target = T> + 'l {
        self.validate_lock(lock);
        // SAFETY: If the lock is valid, there should never be any
        // overlapping mutable references.
        unsafe { self.get_data_unchecked() }
    }

    /// Gets a mutable reference to the data stored in the node `T`
    ///
    /// SAFETY: The caller must ensure that there are no
    /// other references to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_data_mut_unchecked<'l>(&'l self) -> impl core::ops::DerefMut<Target = T> + '_ {
        let n = self.get_node_unchecked();
        n.get_data_mut()
    }

    /// Gets a reference to the data stored in the node `T`
    ///
    /// The supplied `ListLock` must be created from the
    /// registered `(Raw)IntrusiveList`, otherwise it will
    /// panic (only in debug mode).
    #[inline(always)]
    pub fn get_data_mut<'l>(&'l self, lock: &'l mut ListLock) -> impl core::ops::DerefMut<Target = T> + '_ {
        self.validate_lock(lock);

        // SAFETY: If the lock is valid, there should never be any
        // overlapping references.
        unsafe { self.get_data_mut_unchecked() }
    }

    /// Gets a `NodeRef<T>` of the next node in the list.
    ///
    /// SAFETY: The caller must ensure that there are no
    /// other mutable references to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_next_unchecked(&self) -> Option<NodeRef<T>> {
        let n = self.get_node_unchecked();
        *n.get_next()
    }

    /// Gets a `NodeRef<T>` of the next node in the list.
    ///
    /// The supplied `ListLock` must be created from the
    /// registered `(Raw)IntrusiveList`, otherwise it will
    /// panic (only in debug mode).
    #[inline(always)]
    pub fn get_next(&self, lock: &ListLock) -> Option<NodeRef<T>> {
        self.validate_lock(lock);

        // SAFETY: If the lock is valid, there should never be any
        // overlapping references.
        unsafe { self.get_next_unchecked() }
    }

    /// Gets a `NodeRef<T>` to the next node in the list.
    ///
    /// SAFETY: The caller must ensure that there are no
    /// other mutable references to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_next_mut_unchecked<'l>(&'l self) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + 'l {
        let n = self.get_node_unchecked();
        n.get_next_mut()
    }

    /// Gets a `NodeRef<T>` to the next node in the list.
    ///
    /// The supplied `ListLock` must be created from the
    /// registered `(Raw)IntrusiveList`, otherwise it will
    /// panic (only in debug mode).
    #[inline(always)]
    pub fn get_next_mut<'l>(
        &'l self,
        lock: &'l mut ListLock,
    ) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + 'l {
        let n = self.get_node(lock);
        unsafe { n.get_next_mut() }
    }
}

pub struct NodeGuard<'a, T, M: RawMutex> {
    pub(super) lock: Pin<&'a M>,
    pub(super) list: Pin<&'a RawIntrusiveList<T>>,
    pub(super) node: Pin<&'a Node<T>>,
}

impl<'a, T, M: RawMutex> NodeGuard<'a, T, M> {
    pub fn get_ref(&self) -> NodeRef<T> {
        #[cfg(debug_assertions)]
        let list_ptr = self.list.as_ref().get_ref() as *const RawIntrusiveList<T>;
        let ptr = self.node.as_ref().get_ref() as *const _;

        NodeRef {
            #[cfg(debug_assertions)]
            list_ptr: list_ptr.cast(),
            ptr,
        }
    }
}

impl<'a, T, M: RawMutex> Drop for NodeGuard<'a, T, M> {
    fn drop(&mut self) {
        self.lock.lock(|| unsafe {
            self.list.deregister(self);
        });
    }
}
