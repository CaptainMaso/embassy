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

impl<T, M: RawMutex> IntrusiveList<T, M> {
    /// Creates a new intrusive list
    pub const fn new() -> Self {
        Self {
            mutex: M::INIT,
            inner: RawIntrusiveList::new(),
        }
    }

    /// Runs a function with a list lock
    #[inline(always)]
    pub fn with_lock<F, O>(&self, f: F) -> O
    where
        F: FnOnce(&mut ListLock) -> O,
    {
        let mut lock = unsafe { self.lock() };
        self.mutex.lock(|| f(&mut lock))
    }

    pub(crate) unsafe fn lock(&self) -> ListLock<'_> {
        let l = unsafe { self.inner.get_lock() };
        ListLock {
            inner: l,
            _ref: core::marker::PhantomData,
        }
    }

    /// Returns a reference to the head of the list
    #[inline]
    pub(super) fn head(&self, lock: &ListLock<'_>) -> Option<NodeRef<T>> {
        lock.validate(self);
        unsafe { self.inner.head() }
    }

    /// Pushes a node to the head of the list
    #[inline]
    pub fn push_head<'s, 'n, 'g>(&'s self, node: Pin<&'n Node<T>>, lock: &mut ListLock) -> NodeGuard<'g, T, M>
    where
        's: 'g,
        'n: 'g,
    {
        lock.validate(&self);
        unsafe { self.inner.push_head(&self.mutex, node) }
    }

    /// Pushes a node to the tail of the list
    #[inline]
    pub fn push_tail<'s, 'n, 'g>(&'s self, node: Pin<&'n Node<T>>, lock: &mut ListLock) -> NodeGuard<'g, T, M>
    where
        's: 'g,
        'n: 'g,
    {
        lock.validate(&self);
        unsafe { self.inner.push_tail(&self.mutex, node) }
    }

    /// Pushes a node to the position specified.
    ///
    /// Inserts at the end of the list if `index` is greater than the length of the list.
    #[inline]
    pub fn insert<'s, 'n, 'g>(
        &'s self,
        index: usize,
        node: Pin<&'n Node<T>>,
        lock: &mut ListLock,
    ) -> NodeGuard<'g, T, M>
    where
        's: 'g,
        'n: 'g,
    {
        lock.validate(&self);

        let Some(mut cursor) = self.head(lock) else {
            return self.push_head(node, lock);
        };

        if index == 0 {
            return self.push_head(node, lock);
        }

        let mut cur_idx = 0;
        let mut prev = cursor;

        while let Some(next) = cursor.get_next(lock) {
            prev = cursor;
            cursor = next;
            cur_idx += 1;
            if cur_idx == index {
                break;
            }
        }

        let guard = NodeGuard {
            lock: &self.mutex,
            list: &self.inner,
            node,
        };

        let guard_ref = guard.get_ref();

        *prev.get_next_mut(lock) = Some(guard_ref);
        *cursor.get_prev_mut(lock) = Some(guard_ref);

        *guard_ref.get_next_mut(lock) = Some(cursor);
        *guard_ref.get_prev_mut(lock) = Some(prev);

        guard
    }

    #[inline]
    /// Iterates over the list while `f` returns `true`.
    pub fn for_each_while<F>(&self, f: F, lock: &mut ListLock)
    where
        F: FnMut(&mut T) -> bool,
    {
        lock.validate(&self);

        unsafe { self.inner.for_each_while(f) }
    }

    /// Iterates over the list
    #[inline]
    pub fn for_each<F>(&self, f: F, lock: &mut ListLock)
    where
        F: FnMut(&mut T),
    {
        lock.validate(&self);

        unsafe { self.inner.for_each(f) }
    }

    #[inline]
    /// Iterates over the list while `f` returns `true` adding values to the accumulator
    pub fn fold_while<A, F>(&self, init: A, f: F, lock: &mut ListLock) -> A
    where
        F: FnMut(&mut A, &mut T) -> bool,
    {
        lock.validate(&self);
        unsafe { self.inner.fold_while(init, f) }
    }

    /// Iterates over the list while `f` returns `true`.
    #[inline]
    pub fn fold<A, F>(&self, init: A, f: F, lock: &mut ListLock) -> A
    where
        F: FnMut(A, &mut T) -> A,
    {
        lock.validate(&self);
        unsafe { self.inner.fold(init, f) }
    }

    /// Determines if any of the items in the list meet the criteria of `f`
    ///
    /// Short cuts if any items return `true`
    #[inline]
    pub fn any<F>(&self, mut f: F, lock: &mut ListLock) -> bool
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

    /// Determines if all of the items in the list meet the criteria of `f`.
    ///
    /// Short cuts if any items return `false`
    #[inline]
    pub fn all<F>(&self, mut f: F, lock: &mut ListLock) -> bool
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
