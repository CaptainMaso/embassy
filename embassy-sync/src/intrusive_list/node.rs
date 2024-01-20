use core::pin::Pin;

use pin_project::pin_project;

use super::*;
use crate::{blocking_mutex::raw::RawMutex, debug_cell::DebugCell};

#[pin_project]
pub struct Node<T> {
    data: DebugCell<T>,
    prev: DebugCell<Option<NodeRef<T>>>,
    next: DebugCell<Option<NodeRef<T>>>,
}

impl<T> Node<T> {
    pub fn new(data: T) -> Self {
        Self {
            data: DebugCell::new(data),
            prev: DebugCell::new(None),
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

    /// Gets a shared reference to the previous node reference
    ///
    /// SAFETY: Assumes that the caller has a valid shared reference to
    /// the `RawIntrusiveList`.
    unsafe fn get_prev(&self) -> impl core::ops::Deref<Target = Option<NodeRef<T>>> + '_ {
        self.prev.borrow()
    }

    /// Gets a mutable the value of the previous node
    ///
    /// SAFETY: Assumes that the caller has a valid unique reference to the `RawIntrusiveList`
    unsafe fn get_prev_mut(&self) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + '_ {
        self.prev.borrow_mut()
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
    /// # Safety
    ///
    /// - The caller must ensure that there are no other mutable references to the `RawIntrusiveList`.
    /// - The caller must ensure that the particular node that is referenced is still registered to the list
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
    pub fn get_node<'l>(&'l self, lock: &'l ListLock<'_>) -> &'l Node<T> {
        lock.valididate_ptr(self.list_ptr);
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
    pub fn get_data<'l>(&'l self, lock: &'l ListLock<'_>) -> impl core::ops::Deref<Target = T> + 'l {
        lock.valididate_ptr(self.list_ptr);
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
    pub fn get_data_mut<'l>(&'l self, lock: &'l mut ListLock<'_>) -> impl core::ops::DerefMut<Target = T> + '_ {
        lock.valididate_ptr(self.list_ptr);

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
    pub fn get_next(&self, lock: &ListLock<'_>) -> Option<NodeRef<T>> {
        lock.valididate_ptr(self.list_ptr);

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
        lock: &'l mut ListLock<'_>,
    ) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + 'l {
        lock.valididate_ptr(self.list_ptr);
        let n = self.get_node(lock);
        unsafe { n.get_next_mut() }
    }

    /// Gets a `NodeRef<T>` of the previous node in the list.
    ///
    /// SAFETY: The caller must ensure that there are no
    /// other mutable references to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_prev_unchecked(&self) -> Option<NodeRef<T>> {
        let n = self.get_node_unchecked();
        *n.get_prev()
    }

    /// Gets a `NodeRef<T>` of the previous node in the list.
    ///
    /// The supplied `ListLock` must be created from the
    /// registered `(Raw)IntrusiveList`, otherwise it will
    /// panic (only in debug mode).
    #[inline(always)]
    pub fn get_prev(&self, lock: &ListLock<'_>) -> Option<NodeRef<T>> {
        lock.valididate_ptr(self.list_ptr);

        // SAFETY: If the lock is valid, there should never be any
        // overlapping references.
        unsafe { self.get_prev_unchecked() }
    }

    /// Gets a `NodeRef<T>` to the previous node in the list.
    ///
    /// SAFETY: The caller must ensure that there are no
    /// other mutable references to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_prev_mut_unchecked<'l>(&'l self) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + 'l {
        let n = self.get_node_unchecked();
        n.get_prev_mut()
    }

    /// Gets a `NodeRef<T>` to the previous node in the list.
    ///
    /// The supplied `ListLock` must be created from the
    /// registered `(Raw)IntrusiveList`, otherwise it will
    /// panic (only in debug mode).
    #[inline(always)]
    pub fn get_prev_mut<'l>(
        &'l self,
        lock: &'l mut ListLock<'_>,
    ) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + 'l {
        lock.valididate_ptr(self.list_ptr);
        let n = self.get_node(lock);
        unsafe { n.get_prev_mut() }
    }
}

pub struct NodeGuard<'a, T, M: RawMutex> {
    pub(super) lock: &'a M,
    pub(super) list: &'a RawIntrusiveList<T>,
    pub(super) node: Pin<&'a Node<T>>,
}

impl<'a, T, M: RawMutex> NodeGuard<'a, T, M> {
    /// Returns a `NodeRef<T>` pointing at this node
    pub fn get_ref(&self) -> NodeRef<T> {
        #[cfg(debug_assertions)]
        let list_ptr = self.list as *const RawIntrusiveList<T>;
        let ptr = self.node.as_ref().get_ref() as *const _;

        NodeRef {
            #[cfg(debug_assertions)]
            list_ptr: list_ptr.cast(),
            ptr,
        }
    }

    pub fn map<O>(&self, f: impl FnOnce(&mut T) -> O) -> O {
        self.lock.lock(|| {
            let mut d = unsafe { self.node.get_data_mut() };
            f(&mut d)
        })
    }
}

impl<'a, T, M: RawMutex> Drop for NodeGuard<'a, T, M> {
    fn drop(&mut self) {
        let n = self.get_ref();
        self.lock.lock(|| unsafe {
            self.list.deregister(n);
        });
    }
}
