use core::pin::Pin;

use crate::{blocking_mutex::raw::RawMutex, debug_cell::DebugCell};

use super::*;

#[derive(Debug)]
pub struct RawListLock {
    #[allow(dead_code)]
    #[cfg(debug_assertions)]
    list_ptr: *const (),
}

impl RawListLock {
    #[inline(always)]
    pub(super) fn validate<T>(&self, list: &RawIntrusiveList<T>) {
        #[cfg(debug_assertions)]
        {
            let list_ptr = (list as *const RawIntrusiveList<T>).cast();
            self.validate_ptr(list_ptr)
        }
    }

    #[inline(always)]
    pub(super) fn validate_ptr(&self, list_ptr: *const ()) {
        #[cfg(debug_assertions)]
        {
            if self.list_ptr != list_ptr {
                panic!("List lock invalid this list")
            }
        }
    }
}

pub struct RawIntrusiveList<T> {
    head: DebugCell<Option<NodeRef<T>>>,
    tail: DebugCell<Option<NodeRef<T>>>,
}

impl<T> RawIntrusiveList<T> {
    pub const fn new() -> Self {
        Self {
            head: DebugCell::new(None),
            tail: DebugCell::new(None),
        }
    }

    /// Creates a `ListLock` for this `RawIntrusiveList`, so that other operations
    /// can be performed safely.
    ///
    /// SAFETY: Asserts that the caller currently has a unique reference to the
    /// `RawIntrusiveList`.
    #[inline(always)]
    pub const unsafe fn get_lock(&self) -> RawListLock {
        RawListLock {
            #[cfg(debug_assertions)]
            list_ptr: (self as *const Self).cast(),
        }
    }

    /// Returns a reference to the head of the list
    ///
    /// SAFETY: Requires the caller to ensure that no mutable references
    /// exist to the list.
    #[inline]
    pub unsafe fn head(&self) -> Option<NodeRef<T>> {
        *self.head.borrow()
    }

    #[inline]
    pub unsafe fn head_mut<'l>(&'l self) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + 'l {
        self.head.borrow_mut()
    }

    #[inline]
    pub unsafe fn push_head<'a, M: RawMutex>(&'a self, lock: &'a M, node: Pin<&'a Node<T>>) -> NodeGuard<'a, T, M> {
        let guard = NodeGuard { lock, list: self, node };
        let guard_ref = guard.get_ref();
        let old_head = self.head_mut().replace(guard_ref);
        *guard_ref.get_next_mut_unchecked() = old_head;
        if let Some(old_head) = old_head {
            *old_head.get_prev_mut_unchecked() = Some(guard_ref);
        } else {
            *self.tail_mut() = Some(guard_ref);
        }
        guard
    }

    /// Returns a reference to the tail of the list
    ///
    /// SAFETY: Requires the caller to ensure that no mutable references
    /// exist to the list.
    #[inline]
    pub unsafe fn tail(&self) -> Option<NodeRef<T>> {
        *self.tail.borrow()
    }

    #[inline]
    pub unsafe fn tail_mut<'l>(&'l self) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + 'l {
        self.tail.borrow_mut()
    }

    #[inline]
    pub unsafe fn push_tail<'s, 'n, 'g, M: RawMutex>(
        &'s self,
        lock: &'s M,
        node: Pin<&'n Node<T>>,
    ) -> NodeGuard<'g, T, M>
    where
        's: 'g,
        'n: 'g,
    {
        let guard = NodeGuard { lock, list: self, node };
        let guard_ref = guard.get_ref();
        let old_tail = self.tail_mut().replace(guard_ref);
        *guard_ref.get_prev_mut_unchecked() = old_tail;
        if let Some(old_tail) = old_tail {
            *old_tail.get_next_mut_unchecked() = Some(guard_ref);
        } else {
            *self.head_mut() = Some(guard_ref);
        }
        guard
    }

    /// Deregisters the node referred to by `node_ptr`
    ///
    /// # Safety:
    ///  - The caller asserts that it has a mutable lock to the `IntrusiveList`.
    ///  - The caller asserts that the node pointed to by `node_ptr` is still alive.
    ///  - The caller asserts that the nodes pointed to by `node.prev` & `node.next` are still alive.
    #[inline]
    pub unsafe fn deregister(&self, node_ptr: NodeRef<T>) {
        if let Some(next) = node_ptr.get_next_unchecked() {
            *next.get_prev_mut_unchecked() = node_ptr.get_prev_unchecked();
        } else {
            *self.tail_mut() = None;
        }

        if let Some(prev) = node_ptr.get_prev_unchecked() {
            *prev.get_next_mut_unchecked() = node_ptr.get_next_unchecked();
        } else {
            *self.head_mut() = None;
        }
    }

    #[inline]
    /// Iterates over the list while `f` returns `true`.
    pub unsafe fn for_each_while<F>(&self, mut f: F)
    where
        F: FnMut(&mut T) -> bool,
    {
        if let Some(first_node) = self.head() {
            let mut next_node = first_node;
            loop {
                if !f(&mut *next_node.get_data_mut_unchecked()) {
                    break;
                }

                if let Some(next) = next_node.get_next_unchecked() {
                    next_node = next;
                } else {
                    break;
                }
            }
        }
    }

    #[inline]
    pub unsafe fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&mut T),
    {
        if let Some(first_node) = self.head() {
            let mut next_node = first_node;
            loop {
                f(&mut *next_node.get_data_mut_unchecked());

                if let Some(next) = next_node.get_next_unchecked() {
                    next_node = next;
                } else {
                    break;
                }
            }
        }
    }

    #[inline]
    /// Iterates over the list while `f` returns `true` adding values to the accumulator
    pub unsafe fn fold_while<A, F>(&self, mut init: A, mut f: F) -> A
    where
        F: FnMut(&mut A, &mut T) -> bool,
    {
        self.for_each_while(|t| f(&mut init, t));
        init
    }

    #[inline]
    /// Iterates over the list while `f` returns `true`.
    pub unsafe fn fold<A, F>(&self, init: A, mut f: F) -> A
    where
        F: FnMut(A, &mut T) -> A,
    {
        let mut a = Some(init);
        self.for_each(|t| {
            a = Some(f(a.take().unwrap(), t));
        });
        a.unwrap()
    }
}
