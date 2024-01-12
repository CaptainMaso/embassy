use core::pin::Pin;

use crate::{blocking_mutex::raw::RawMutex, debug_cell::DebugCell};

use super::{raw::ListLock, *};

pub struct IntrusiveList<T, M> {
    mutex: M,
    inner: RawIntrusiveList<T>,
}

impl<T, M: RawMutex> IntrusiveList<T, M> {
    pub const fn new() -> Self {
        Self {
            mutex: M::INIT,
            inner: RawIntrusiveList::new(),
        }
    }

    #[inline(always)]
    pub fn with_lock<F, O>(&self, mut f: F) -> O
    where
        F: FnMut(&mut ListLock) -> O,
    {
        self.mutex.lock(|| {
            let mut l = unsafe { self.inner.get_lock() };
            f(&mut l)
        })
    }

    /// Returns a reference to the head of the list
    #[inline]
    pub fn head<'l>(&'l self, lock: &'l ListLock) -> impl core::ops::Deref<Target = Option<NodeRef<T>>> + 'l {
        self.validate_lock(lock);
        unsafe { self.inner.head() }
    }

    #[inline]
    fn head_mut<'l>(&'l self, lock: &'l mut ListLock) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + 'l {
        self.validate_lock(lock);
        unsafe { self.inner.head_mut() }
    }

    #[inline]
    pub fn push_head<'a>(self: Pin<&'a Self>, node: Pin<&'a Node<T>>) -> NodeGuard<'a, T, M> {
        let guard = self.with_lock(move |lock| {
            let guard = NodeGuard { list: self, node };
            let old_node = self.head_mut(lock).replace(guard.get_ref());
            *guard.get_ref().get_next_mut(lock) = old_node;
            guard
        });
        guard
    }

    #[inline]
    pub fn push_tail<'a>(self: Pin<&'a Self>, node: Pin<&'a Node<T>>) -> NodeGuard<'a, T> {
        let guard = self.with_lock(move |lock| {
            let guard = NodeGuard { list: self, node };
            let first_node = { *self.head(lock) };
            if let Some(first_node) = first_node {
                let mut next_node = first_node;
                loop {
                    let next = { *next_node.get_next(lock) };
                    if let Some(next) = next {
                        next_node = next;
                    } else {
                        *next_node.get_next_mut(lock) = Some(guard.get_ref());
                        break;
                    }
                }
            } else {
                *self.head_mut(lock) = Some(guard.get_ref());
            }
            guard
        });
        guard
    }

    #[inline]
    pub(super) fn deregister(self: Pin<&Self>, node: &mut NodeGuard<'_, T>) {
        let node_ptr = node.get_ref();

        self.with_lock(|lock| {
            let first_node = { *self.head(lock) };
            if let Some(first_node) = first_node {
                let mut prev_node = None;
                let mut next_node = first_node;
                let prev_node = loop {
                    if next_node == node_ptr {
                        break prev_node;
                    }

                    let next = { *next_node.get_next(lock) };
                    if let Some(next) = next {
                        prev_node = Some(next_node);
                        next_node = next;
                    } else {
                        // Reached end without finding node
                        return;
                    }
                };

                if let Some(mut prev_node) = prev_node {
                    *prev_node.get_next_mut(lock) = None;
                } else {
                    *self.head_mut(lock) = None;
                }
            }
        });
    }

    #[inline]
    /// Iterates over the list while `f` returns `true`.
    pub fn for_each_while<F>(self: Pin<&Self>, mut f: F)
    where
        F: FnMut(&mut T) -> bool,
    {
        self.with_lock(|lock| {
            if let Some(first_node) = self.head(lock).as_ref().copied() {
                let mut next_node = first_node;
                loop {
                    if !f(&mut *next_node.get_data_mut(lock)) {
                        break;
                    }

                    let next = { *next_node.get_next(lock) };
                    if let Some(next) = next {
                        next_node = next;
                    } else {
                        break;
                    }
                }
            }
        });
    }

    #[inline]
    pub fn for_each<F>(self: Pin<&Self>, mut f: F)
    where
        F: FnMut(&mut T),
    {
        self.with_lock(|lock| {
            if let Some(first_node) = self.head(lock).as_ref().copied() {
                let mut next_node = first_node;
                loop {
                    f(&mut *next_node.get_data_mut(lock));

                    let next = { *next_node.get_next(lock) };
                    if let Some(next) = next {
                        next_node = next;
                    } else {
                        break;
                    }
                }
            }
        });
    }

    #[inline]
    /// Iterates over the list while `f` returns `true` adding values to the accumulator
    pub fn fold_while<A, F>(self: Pin<&Self>, mut init: A, mut f: F) -> A
    where
        F: FnMut(&mut A, &mut T) -> bool,
    {
        self.for_each_while(|t| f(&mut init, t));
        init
    }

    #[inline]
    /// Iterates over the list while `f` returns `true`.
    pub fn fold<A, F>(self: Pin<&Self>, init: A, mut f: F) -> A
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
