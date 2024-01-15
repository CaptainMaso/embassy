use core::pin::Pin;

use crate::{blocking_mutex::raw::RawMutex, debug_cell::DebugCell};

use super::*;

#[derive(Debug)]
pub(super) struct RawListLock {
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
}

impl<T> RawIntrusiveList<T> {
    pub const fn new() -> Self {
        Self {
            head: DebugCell::new(None),
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
    pub unsafe fn push_head<'a, M: RawMutex>(
        self: Pin<&'a Self>,
        lock: Pin<&'a M>,
        node: Pin<&'a Node<T>>,
    ) -> NodeGuard<'a, T, M> {
        let guard = NodeGuard { lock, list: self, node };
        let old_node = self.head_mut().replace(guard.get_ref());
        *guard.get_ref().get_next_mut_unchecked() = old_node;
        guard
    }

    #[inline]
    pub unsafe fn push_tail<'a, M: RawMutex>(
        self: Pin<&'a Self>,
        lock: Pin<&'a M>,
        node: Pin<&'a Node<T>>,
    ) -> NodeGuard<'a, T, M> {
        let guard = NodeGuard { list: self, lock, node };

        if let Some(first_node) = self.head() {
            let mut next_node = first_node;
            loop {
                if let Some(next) = next_node.get_next_unchecked() {
                    next_node = next;
                } else {
                    *next_node.get_next_mut_unchecked() = Some(guard.get_ref());
                    break;
                }
            }
        } else {
            *self.head_mut() = Some(guard.get_ref());
        }
        guard
    }

    #[inline]
    pub(super) unsafe fn deregister<M: RawMutex>(self: Pin<&Self>, node: &mut NodeGuard<'_, T, M>) {
        let node_ptr = node.get_ref();

        let first_node = self.head();
        if let Some(first_node) = first_node {
            let mut prev_node = None;
            let mut next_node = first_node;
            let prev_node = loop {
                if next_node == node_ptr {
                    break prev_node;
                }

                if let Some(next) = next_node.get_next_unchecked() {
                    prev_node = Some(next_node);
                    next_node = next;
                } else {
                    // Reached end without finding node
                    return;
                }
            };

            if let Some(mut prev_node) = prev_node {
                *prev_node.get_next_mut_unchecked() = None;
            } else {
                *self.head_mut() = None;
            }
        }
    }

    #[inline]
    /// Iterates over the list while `f` returns `true`.
    pub unsafe fn for_each_while<F>(self: Pin<&Self>, mut f: F)
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
    pub unsafe fn for_each<F>(self: Pin<&Self>, mut f: F)
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
    pub unsafe fn fold_while<A, F>(self: Pin<&Self>, mut init: A, mut f: F) -> A
    where
        F: FnMut(&mut A, &mut T) -> bool,
    {
        self.for_each_while(|t| f(&mut init, t));
        init
    }

    #[inline]
    /// Iterates over the list while `f` returns `true`.
    pub unsafe fn fold<A, F>(self: Pin<&Self>, init: A, mut f: F) -> A
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
