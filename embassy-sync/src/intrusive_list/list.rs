use core::marker::PhantomData;
use core::ops::ControlFlow;
use core::pin::Pin;

use self::raw::RawCursor;
use super::*;
use crate::blocking_mutex::raw::{ConstRawMutex, RawMutex};
use crate::blocking_mutex::Mutex;
use crate::debug_cell::DebugCell;

pub struct IntrusiveList<T: ?Sized, M: RawMutex> {
    inner: Mutex<M, DebugCell<RawIntrusiveList>>,
    _data: PhantomData<T>,
}

unsafe impl<T: ?Sized, M: RawMutex> Sync for IntrusiveList<T, M> {}
unsafe impl<T: ?Sized, M: RawMutex> Send for IntrusiveList<T, M> {}

impl<T, M: RawMutex> IntrusiveList<T, M> {
    /// Creates a new intrusive list
    pub const fn new() -> Self
    where
        M: ConstRawMutex,
    {
        Self {
            inner: Mutex::new(DebugCell::new(RawIntrusiveList::new())),
            _data: PhantomData,
        }
    }

    pub const fn new_store(&self, item: T) -> Item<'_, T, M> {
        Item {
            node: Node::new(),
            data: DebugCell::new(item),
            list: self,
        }
    }

    pub fn with_cursor<O, F>(&self, f: F) -> O
    where
        F: FnOnce(&mut Cursor<'_, T, M>) -> O,
    {
        self.inner.lock(|l| {
            let mut l = unsafe { l.borrow_mut() };

            let mut cursor = Cursor {
                m: PhantomData,
                inner: l.cursor(),
            };

            f(&mut cursor)
        })
    }

    /// Removes the specified item from the list, without needing the look up the item directly
    #[inline]
    pub fn remove(&self, item: Pin<&Item<'_, T, M>>) {
        self.inner.lock(|i| unsafe { i.borrow_mut().remove(item.node()) })
    }
}

#[pin_project::pin_project(PinnedDrop)]
#[repr(C)]
pub struct Item<'a, T, M: RawMutex> {
    #[pin]
    node: Node,
    #[pin]
    data: DebugCell<T>,
    list: &'a IntrusiveList<T, M>,
}

impl<'a, T, M: RawMutex> Item<'a, T, M> {
    #[inline]
    pub fn with_cursor<O, F>(self: Pin<&Self>, f: F) -> O
    where
        F: FnOnce(&mut Cursor<'_, T, M>) -> O,
    {
        self.list.with_cursor(f)
    }

    #[inline]
    pub fn lock<O, F>(self: Pin<&Self>, f: F) -> O
    where
        F: FnOnce(&mut T) -> O,
    {
        self.list.with_cursor(|c| {
            // Safety: If we have the cursor, we can safely get access to all data in the list.
            let mut item_ref = unsafe { self.node.get_data_mut() };
            f(&mut *item_ref)
        })
    }

    #[inline]
    pub fn is_in_list(self: Pin<&Self>) -> bool {
        self.list.with_cursor(|_c| unsafe { self.node.get_links().is_linked() })
    }

    #[inline]
    pub fn remove(self: Pin<&Self>) {
        self.list.remove(self);
    }
}

#[pin_project::pinned_drop]
impl<T, M: RawMutex> PinnedDrop for Item<'_, T, M> {
    fn drop(self: Pin<&mut Self>) {
        self.list.remove(self.as_ref());
    }
}

pub struct Cursor<'a, T, M> {
    _m: core::marker::PhantomData<&'a M>,
    _t: core::marker::PhantomData<&'a T>,
    inner: RawCursor<'a>,
}

impl<'a, T, M: RawMutex> Cursor<'a, T, M> {
    pub fn get(&mut self) -> Option<crate::debug_cell::RefMut<'_, T>> {
        self.inner.get_mut()
    }

    /// Pushes a node to the head of the list
    #[inline]
    pub fn insert_head(&mut self, item: Pin<&Item<'_, T, M>>) {
        self.inner.insert_head(item.node());
    }

    /// Pushes a node to the tail of the list
    #[inline]
    pub fn insert_tail(&mut self, item: Pin<&Item<'_, T, M>>) {
        self.inner.insert_tail(item.node());
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.inner.idx()
    }

    #[inline]
    fn inner_fold<B, C, F>(&mut self, init: C, mut f: F) -> ControlFlow<B, C>
    where
        F: FnMut(C, usize, &mut T) -> ControlFlow<B, C>,
    {
        self.inner.current();
        if self.is_empty() {
            return ControlFlow::Continue(init);
        }

        let mut acc = core::mem::MaybeUninit::new(init);
        loop {
            let index = self.index();
            {
                let Some(mut r) = self.inner.get_mut() else {
                    unreachable!()
                };
                let next = f(unsafe { acc.assume_init_read() }, index, &mut *r)?;
                acc.write(next);
            }
            if self.inner.is_tail() {
                break ControlFlow::Continue(unsafe { acc.assume_init_read() });
            }
            self.inner.next();
        }
    }

    #[inline]
    fn seek(&self, index: usize) -> SeekFrom {
        let current_idx = self.inner.idx();

        let diff_current = current_idx as isize - index as isize;

        if index == 0 {
            return SeekFrom::Head(0);
        } else if index >= self.len() {
            return SeekFrom::Tail(0);
        } else if diff_current == 0 {
            return SeekFrom::Current(0);
        }

        let abs_diff_current = SeekFrom::Current(diff_current);
        let diff_start = SeekFrom::Head(index);
        let diff_end = SeekFrom::Head(self.inner.len() - index);
        SeekFrom::find_min([abs_diff_current, diff_start, diff_end]).unwrap()
    }

    /// Sets the cursor to the position specified.
    ///
    /// This move is saturated on the tail.
    #[inline]
    pub fn set_index(&mut self, index: usize) {
        let (steps, is_after) = match self.seek(index) {
            SeekFrom::Head(v) => {
                self.inner.head();
                (v, true)
            }
            SeekFrom::Tail(v) => {
                self.inner.tail();
                (v, false)
            }
            SeekFrom::Current(v) => (v.unsigned_abs(), v >= 0),
        };
        if is_after {
            for _ in 0..steps {
                self.inner.next();
            }
        } else {
            for _ in 0..steps {
                self.inner.prev();
            }
        }
    }

    /// Pushes a node to the position specified.
    ///
    /// Inserts at the end of the list if `index` is greater than the length of the list.
    #[inline]
    pub fn insert(&mut self, index: usize, item: Pin<&Item<'_, T, M>>) {
        let (steps, is_after) = match self.seek(index) {
            SeekFrom::Head(v) => {
                self.inner.head();
                (v, v != 0)
            }
            SeekFrom::Tail(v) => {
                self.inner.tail();
                (v, v != 0)
            }
            SeekFrom::Current(v) => (v.unsigned_abs(), v > 0),
        };
        let steps = steps.saturating_sub(1);
        if is_after {
            for _ in 0..steps {
                self.inner.next();
            }
            self.inner.insert_after(item.node());
        } else {
            for _ in 0..steps {
                self.inner.prev();
            }
            self.inner.insert_before(item.node());
        }
    }

    /// Inserts an item before the item that returns `true`, returning the index that the item was inserted at.
    ///
    /// `f` : `FnMut(current_index, current_item, inserted_item) -> bool`
    ///
    /// If no item is found, inserts at the head
    #[inline]
    pub fn insert_before<F>(&mut self, item: Pin<&Item<'_, T, M>>, mut f: F) -> usize
    where
        F: FnMut(usize, &mut T, &mut T) -> bool,
    {
        // Safety: by inserted the node, the user cannot directly access that data any longer
        // and we hold the lock to the list.
        let pos = {
            let item_node = item.node();
            let mut node_ref = unsafe { item_node.get_data_mut() };
            self.position(|idx, item| f(idx, item, &mut *node_ref))
        };

        if let Some(p) = pos {
            let p = p.saturating_sub(1);
            self.insert(p, item);
            p
        } else {
            self.insert_head(item);
            0
        }
    }

    /// Inserts an item after the item that returns `true`, returning the inserted index
    ///
    /// `f` : `FnMut(current_index, current_item, inserted_item) -> bool`
    ///
    /// If no item is found, inserts at the tail
    #[inline]
    pub fn insert_after<F>(&mut self, item: Pin<&Item<'_, T, M>>, mut f: F) -> usize
    where
        F: FnMut(usize, &mut T, &mut T) -> bool,
    {
        // Safety: by inserted the node, the user cannot directly access that data any longer
        // and we hold the lock to the list.
        let pos = {
            let item_node = item.node();
            let mut node_ref = unsafe { item_node.get_data_mut() };
            self.position(|idx, item| f(idx, item, &mut *node_ref))
        };

        if let Some(p) = pos {
            self.insert(p + 1, item);
            p + 1
        } else {
            self.insert_tail(item);
            self.len() - 1
        }
    }

    /// Iterates over the list until one item returns `Err(E)`
    #[inline]
    pub fn try_for_each<E, F>(&mut self, mut f: F) -> Result<(), E>
    where
        F: FnMut(usize, &mut T) -> Result<(), E>,
    {
        let r = self.inner_fold((), |_, usize, t| {
            if let Err(e) = f(usize, t) {
                ControlFlow::Break(e)
            } else {
                ControlFlow::Continue(())
            }
        });
        if let ControlFlow::Break(e) = r {
            Err(e)
        } else {
            Ok(())
        }
    }

    /// Iterates over the list
    #[inline]
    pub fn for_each<F>(&mut self, mut f: F)
    where
        F: FnMut(usize, &mut T),
    {
        self.inner_fold((), |_, u, t| {
            f(u, t);
            ControlFlow::Continue::<()>(())
        });
    }

    #[inline]
    /// Iterates over the list while `f` returns `true` adding values to the accumulator
    pub fn try_fold<A, E, F>(&mut self, init: A, mut f: F) -> Result<A, E>
    where
        F: FnMut(A, usize, &mut T) -> Result<A, E>,
    {
        let res = self.inner_fold(init, |acc, idx, t| match f(acc, idx, t) {
            Ok(acc) => ControlFlow::Continue(acc),
            Err(e) => ControlFlow::Break(e),
        });

        match res {
            ControlFlow::Continue(a) => Ok(a),
            ControlFlow::Break(e) => Err(e),
        }
    }

    /// Iterates over the list, accumulating a value
    #[inline]
    pub fn fold<A, F>(&mut self, init: A, mut f: F) -> A
    where
        F: FnMut(A, usize, &mut T) -> A,
    {
        let mut acc = core::mem::MaybeUninit::new(init);

        self.for_each(|idx, t| {
            let next = f(unsafe { acc.assume_init_read() }, idx, t);
            acc.write(next);
        });

        unsafe { acc.assume_init_read() }
    }

    /// Determines if any of the items in the list meet the criteria of `f`
    ///
    /// Short cuts if any items return `true`.
    ///
    /// If the list is empty, returns `false`.
    #[inline]
    pub fn any<F>(&mut self, mut f: F) -> bool
    where
        F: FnMut(usize, &mut T) -> bool,
    {
        self.inner_fold((), |(), idx, t| {
            if f(idx, t) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .is_break()
    }

    /// Determines if all of the items in the list meet the criteria of `f`.
    ///
    /// Short cuts if any items return `false`.
    ///
    /// If the list is empty, returns `true`.
    #[inline]
    pub fn all<F>(&mut self, mut f: F) -> bool
    where
        F: FnMut(usize, &mut T) -> bool,
    {
        self.inner_fold((), |(), idx, t| {
            if f(idx, t) {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        })
        .is_continue()
    }

    /// Determines the position of the item that meets the criteria of `f`
    #[inline]
    pub fn find<O, F>(&mut self, mut f: F) -> Option<O>
    where
        F: FnMut(usize, &mut T) -> Option<O>,
    {
        let r = self.inner_fold((), |(), idx, t| {
            if let Some(o) = f(idx, t) {
                ControlFlow::Break(o)
            } else {
                ControlFlow::Continue(())
            }
        });

        if let ControlFlow::Break(o) = r {
            Some(o)
        } else {
            None
        }
    }

    /// Determines the position of the item that meets the criteria of `f`
    #[inline]
    pub fn position<F>(&mut self, mut f: F) -> Option<usize>
    where
        F: FnMut(usize, &mut T) -> bool,
    {
        let r = self.inner_fold((), |(), idx, t| {
            if f(idx, t) {
                ControlFlow::Break(idx)
            } else {
                ControlFlow::Continue(())
            }
        });

        if let ControlFlow::Break(o) = r {
            Some(o)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SeekFrom {
    Head(usize),
    Tail(usize),
    Current(isize),
}

impl SeekFrom {
    #[inline]
    fn find_min(i: impl IntoIterator<Item = Self>) -> Option<Self> {
        let mut i = i.into_iter();

        let mut min = i.next()?;

        for n in i {
            let update = match (n, min) {
                (SeekFrom::Head(a), SeekFrom::Head(b))
                | (SeekFrom::Head(a), SeekFrom::Tail(b))
                | (SeekFrom::Tail(a), SeekFrom::Head(b))
                | (SeekFrom::Tail(a), SeekFrom::Tail(b)) => a < b,
                (SeekFrom::Current(c), SeekFrom::Head(b)) | (SeekFrom::Current(c), SeekFrom::Tail(b)) => {
                    c.unsigned_abs() <= b
                }
                (SeekFrom::Current(c), SeekFrom::Current(b)) => c.abs() < b.abs(),
                (SeekFrom::Head(a), SeekFrom::Current(c)) | (SeekFrom::Tail(a), SeekFrom::Current(c)) => {
                    a < c.unsigned_abs()
                }
            };

            if update {
                min = n;
            }
        }

        Some(min)
    }
}
