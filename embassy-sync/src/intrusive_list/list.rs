use core::pin::Pin;

use self::raw::RawCursor;
use super::*;
use crate::blocking_mutex::raw::{ConstRawMutex, RawMutex};
use crate::blocking_mutex::Mutex;
use crate::debug_cell::DebugCell;

pub struct IntrusiveList<T, M: RawMutex> {
    inner: Mutex<M, DebugCell<RawIntrusiveList<T>>>,
}

impl<T, M: RawMutex> IntrusiveList<T, M> {
    /// Creates a new intrusive list
    pub const fn new() -> Self
    where
        M: ConstRawMutex,
    {
        Self {
            inner: Mutex::new(DebugCell::new(RawIntrusiveList::new())),
        }
    }

    pub const fn new_store(&self, item: T) -> Item<'_, T, M> {
        Item {
            node: Node::new(item),
            list: self,
        }
    }

    fn with_cursor<O, F>(&self, f: F) -> O
    where
        F: FnOnce(&mut RawCursor<'_, T>) -> O,
    {
        self.inner.lock(|l| {
            let mut l = unsafe { l.borrow_mut() };

            let mut cursor = l.cursor();

            f(&mut cursor)
        })
    }

    pub fn remove(&self, item: &Item<'_, T, M>) {
        self.inner.lock(|i| unsafe { i.borrow_mut().remove(item.node()) })
    }
}

#[pin_project::pin_project(PinnedDrop)]
pub struct Item<'a, T, M: RawMutex> {
    #[pin]
    node: Node<T>,
    list: &'a IntrusiveList<T, M>,
}

impl<'a, T, M: RawMutex> Item<'a, T, M> {
    #[inline(always)]
    fn node(&self) -> NodeRef<T> {
        let ptr = (&self.node) as *const Node<T>;
        NodeRef { ptr }
    }

    pub fn pin(&self) -> ItemRef<'_, T> {
        let p = Pin::new(self);
        let p = p.project_ref().node;
        ItemRef { p }
    }
}

#[pin_project::pinned_drop]
impl<'a, T, M: RawMutex> PinnedDrop for Item<'a, T, M> {
    fn drop(self: Pin<&mut Self>) {
        self.list.remove(self.as_ref().get_ref());
    }
}

pub struct ItemRef<'a, T> {
    p: Pin<&'a Node<T>>,
}

impl<'a, T> ItemRef<'a, T> {
    #[inline]
    fn node(&self) -> NodeRef<T> {
        let ptr = self.p.get_ref() as *const Node<T>;
        NodeRef { ptr }
    }
}

pub struct Cursor<'a, T> {
    inner: RawCursor<'a, T>,
}

impl<'a, T> Cursor<'a, T> {
    // /// Pushes a node to the head of the list
    #[inline]
    pub fn insert_head(&mut self, item: Pin<&ItemRef<'a, T>>) {
        self.inner.insert_head(item.node());
    }

    // /// Pushes a node to the tail of the list
    // #[inline]
    // pub fn push_tail<'s, 'n, 'g>(&'s self, node: Pin<&'n Node<T>>, lock: &mut ListLock) -> NodeGuard<'g, T, M>
    // where
    //     's: 'g,
    //     'n: 'g,
    // {
    //     lock.validate(&self);
    //     unsafe { self.inner.push_tail(&self.mutex, node) }
    // }

    // /// Pushes a node to the position specified.
    // ///
    // /// Inserts at the end of the list if `index` is greater than the length of the list.
    // #[inline]
    // pub fn insert<'s, 'n, 'g>(
    //     &'s self,
    //     index: usize,
    //     node: Pin<&'n Node<T>>,
    //     lock: &mut ListLock,
    // ) -> NodeGuard<'g, T, M>
    // where
    //     's: 'g,
    //     'n: 'g,
    // {
    //     lock.validate(&self);

    //     let Some(mut cursor) = self.head(lock) else {
    //         return self.push_head(node, lock);
    //     };

    //     if index == 0 {
    //         return self.push_head(node, lock);
    //     }

    //     let mut cur_idx = 0;
    //     let mut prev = cursor;

    //     while let Some(next) = cursor.get_next(lock) {
    //         prev = cursor;
    //         cursor = next;
    //         cur_idx += 1;
    //         if cur_idx == index {
    //             break;
    //         }
    //     }

    //     let guard = NodeGuard {
    //         lock: &self.mutex,
    //         list: &self.inner,
    //         node,
    //     };

    //     let guard_ref = guard.get_ref();

    //     *prev.get_next_mut(lock) = Some(guard_ref);
    //     *cursor.get_prev_mut(lock) = Some(guard_ref);

    //     *guard_ref.get_next_mut(lock) = Some(cursor);
    //     *guard_ref.get_prev_mut(lock) = Some(prev);

    //     guard
    // }

    // #[inline]
    // /// Iterates over the list while `f` returns `true`.
    // pub fn for_each_while<F>(&self, f: F, lock: &mut ListLock)
    // where
    //     F: FnMut(&mut T) -> bool,
    // {
    //     lock.validate(&self);

    //     unsafe { self.inner.for_each_while(f) }
    // }

    // /// Iterates over the list
    // #[inline]
    // pub fn for_each<F>(&self, f: F, lock: &mut ListLock)
    // where
    //     F: FnMut(&mut T),
    // {
    //     lock.validate(&self);

    //     unsafe { self.inner.for_each(f) }
    // }

    // #[inline]
    // /// Iterates over the list while `f` returns `true` adding values to the accumulator
    // pub fn fold_while<A, F>(&self, init: A, f: F, lock: &mut ListLock) -> A
    // where
    //     F: FnMut(&mut A, &mut T) -> bool,
    // {
    //     lock.validate(&self);
    //     unsafe { self.inner.fold_while(init, f) }
    // }

    // /// Iterates over the list while `f` returns `true`.
    // #[inline]
    // pub fn fold<A, F>(&self, init: A, f: F, lock: &mut ListLock) -> A
    // where
    //     F: FnMut(A, &mut T) -> A,
    // {
    //     lock.validate(&self);
    //     unsafe { self.inner.fold(init, f) }
    // }

    // /// Determines if any of the items in the list meet the criteria of `f`
    // ///
    // /// Short cuts if any items return `true`
    // #[inline]
    // pub fn any<F>(&self, mut f: F, lock: &mut ListLock) -> bool
    // where
    //     F: FnMut(&mut T) -> bool,
    // {
    //     self.fold_while(
    //         false,
    //         |acc, n| {
    //             let r = f(n);
    //             *acc = r;
    //             r
    //         },
    //         lock,
    //     )
    // }

    // /// Determines if all of the items in the list meet the criteria of `f`.
    // ///
    // /// Short cuts if any items return `false`
    // #[inline]
    // pub fn all<F>(&self, mut f: F, lock: &mut ListLock) -> bool
    // where
    //     F: FnMut(&mut T) -> bool,
    // {
    //     self.fold_while(
    //         true,
    //         |acc, n| {
    //             let r = f(n);
    //             *acc = r;
    //             !r
    //         },
    //         lock,
    //     )
    // }
}
