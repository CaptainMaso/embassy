use core::pin::Pin;

use crate::blocking_mutex::raw::RawMutex;

use super::*;
use raw::RawListLock;

pub struct ListLock<'a> {
    inner: RawListLock,
    _ref: core::marker::PhantomData<&'a ()>,
}

impl<'a> ListLock<'a> {
    pub fn validate<T, M>(&self, list: &IntrusiveList<T, M>) {
        self.inner.validate(&list.inner)
    }

    pub(super) fn valididate_ptr(&self, list_ptr: *const ()) {
        self.inner.validate_ptr(list_ptr)
    }
}

pub struct IntrusiveList<T, M> {
    mutex: M,
    inner: RawIntrusiveList<T>,
}

struct ProjIntrusiveList<'a, T, M> {
    mutex: Pin<&'a M>,
    inner: Pin<&'a RawIntrusiveList<T>>,
}

impl<T, M: RawMutex> IntrusiveList<T, M> {
    pub const fn new() -> Self {
        Self {
            mutex: M::INIT,
            inner: RawIntrusiveList::new(),
        }
    }

    fn proj(self: Pin<&Self>) -> ProjIntrusiveList<'_, T, M> {
        unsafe {
            ProjIntrusiveList {
                mutex: Pin::new_unchecked(&self.mutex),
                inner: Pin::new_unchecked(&self.inner),
            }
        }
    }

    fn node_guard<'a>(self: Pin<&'a Self>, node: Pin<&'a Node<T>>) -> NodeGuard<'a, T, M> {
        NodeGuard {
            list: unsafe { self.map_unchecked(|s| &s.inner) },
            node,
            lock: unsafe { self.map_unchecked(|s| &s.mutex) },
        }
    }

    #[inline(always)]
    pub fn with_lock<F, O>(self: Pin<&Self>, mut f: F) -> O
    where
        F: FnMut(&mut ListLock) -> O,
    {
        let mut lock = unsafe { self.lock() };
        self.mutex.lock(|| f(&mut lock))
    }

    pub unsafe fn lock(self: Pin<&Self>) -> ListLock<'_> {
        let l = unsafe { self.inner.get_lock() };
        ListLock {
            inner: l,
            _ref: core::marker::PhantomData,
        }
    }

    /// Returns a reference to the head of the list
    #[inline]
    pub fn head(&self, lock: &ListLock<'_>) -> Option<NodeRef<T>> {
        lock.validate(self);
        unsafe { self.inner.head() }
    }

    #[inline]
    fn head_mut<'l>(
        &'l self,
        lock: &'l mut ListLock<'_>,
    ) -> impl core::ops::DerefMut<Target = Option<NodeRef<T>>> + 'l {
        lock.validate(self);
        unsafe { self.inner.head_mut() }
    }

    #[inline]
    pub fn push_head<'a>(self: Pin<&'a Self>, node: Pin<&'a Node<T>>, lock: &mut ListLock) -> NodeGuard<'a, T, M> {
        lock.validate(&self);
        let proj = self.proj();
        unsafe { proj.inner.push_head(proj.mutex, node) }
    }

    #[inline]
    pub fn push_tail<'a>(self: Pin<&'a Self>, node: Pin<&'a Node<T>>, lock: &mut ListLock) -> NodeGuard<'a, T, M> {
        lock.validate(&self);
        let proj = self.proj();
        unsafe { proj.inner.push_tail(proj.mutex, node) }
    }

    #[inline]
    pub(super) fn deregister(self: Pin<&Self>, node: &mut NodeGuard<'_, T, M>, lock: &mut ListLock) {
        lock.validate(&self);
        let proj = self.proj();
        unsafe {
            proj.inner.deregister(node);
        }
    }

    #[inline]
    /// Iterates over the list while `f` returns `true`.
    pub fn for_each_while<F>(self: Pin<&Self>, mut f: F, lock: &mut ListLock)
    where
        F: FnMut(&mut T) -> bool,
    {
        lock.validate(&self);

        let proj = self.proj();
        unsafe { proj.inner.for_each_while(f) }
    }

    #[inline]
    pub fn for_each<F>(self: Pin<&Self>, mut f: F, lock: &mut ListLock)
    where
        F: FnMut(&mut T),
    {
        lock.validate(&self);

        let proj = self.proj();
        unsafe { proj.inner.for_each(f) }
    }

    #[inline]
    /// Iterates over the list while `f` returns `true` adding values to the accumulator
    pub fn fold_while<A, F>(self: Pin<&Self>, mut init: A, mut f: F, lock: &mut ListLock) -> A
    where
        F: FnMut(&mut A, &mut T) -> bool,
    {
        let proj = self.proj();
        unsafe { proj.inner.fold_while(init, f) }
    }

    #[inline]
    /// Iterates over the list while `f` returns `true`.
    pub fn fold<A, F>(self: Pin<&Self>, init: A, mut f: F, lock: &mut ListLock) -> A
    where
        F: FnMut(A, &mut T) -> A,
    {
        let proj = self.proj();
        unsafe { proj.inner.fold(init, f) }
    }

    pub fn any<F>(self: Pin<&Self>, mut f: F, lock: &mut ListLock) -> bool
    where
        F: FnMut(&mut T) -> bool,
    {
        self.fold_while(
            false,
            |acc, n| {
                let r = f(n);
                *acc = r;
                r
            },
            lock,
        )
    }

    pub fn all<F>(self: Pin<&Self>, mut f: F, lock: &mut ListLock) -> bool
    where
        F: FnMut(&mut T) -> bool,
    {
        self.fold_while(
            true,
            |acc, n| {
                let r = f(n);
                *acc = r;
                !r
            },
            lock,
        )
    }
}
