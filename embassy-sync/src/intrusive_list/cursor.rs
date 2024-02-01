use core::ops::ControlFlow;
use core::pin::Pin;

use super::*;
use crate::blocking_mutex::raw::RawMutex;
use crate::debug_cell::DebugRefMut;

pub struct Cursor<'a, T, M> {
    _m: core::marker::PhantomData<&'a M>,
    _t: core::marker::PhantomData<&'a T>,
    list: &'a mut RawIntrusiveList,
    index: usize,
    current: Option<NodePtr>,
}

impl<'a, T, M: RawMutex> Cursor<'a, T, M> {
    fn get_cursor(&mut self) -> Option<Pin<&'a Node>> {
        unsafe {
            Some(self.current?.get())
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.index
    }

    #[inline]
    pub fn get(&mut self) -> Option<DebugRefMut<'_, T>> {
        unsafe {
            let ptr = self.get_cursor()?;
            let n = ItemData::from_node(ptr);
            Some(n.data.borrow_mut())
        }
    }

    #[inline]
    pub fn is_head(&self) -> bool {
        if let Some(c) = self.current {
            unsafe { c.get().as_links().is_head() }
        } else {
            false
        }
    }

    #[inline]
    pub fn is_tail(&self) -> bool {
        if let Some(c) = self.current {
            unsafe { c.get().as_links().is_tail() }
        } else {
            false
        }
    }

    #[inline]
    pub fn seek_head(&mut self) {
        self.current = self.list.head;
        self.index = 0;
    }

    #[inline]
    pub fn seek_tail(&mut self) {
        self.current = self.list.tail;
        self.index = self.list.len.saturating_sub(1);
    }

    /// Moves the cursor to the next item, wrapping around to the head
    #[inline]
    pub fn seek_next(&mut self) {
        if let Some(next) = self.current.and_then(|n| unsafe { n.get().next().expect_linked() }) {
            self.current = Some(next);
            self.index += 1;
        } else {
            self.seek_head()
        }
    }

    /// Moves the cursor to the previous item, wrapping around to the tail
    #[inline]
    pub fn seek_prev(&mut self) {
        if let Some(prev) = self.current.and_then(|n| unsafe { n.get().prev().expect_linked() }) {
            self.current = Some(prev);
            self.index = self.index.saturating_sub(1);
        } else {
            self.seek_tail()
        }
    }

    #[inline]
    fn min_seek(&self, index: usize) -> SeekFrom {
        let current_idx = self.index;

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
        let diff_end = SeekFrom::Head(self.len() - index);
        SeekFrom::find_min([abs_diff_current, diff_start, diff_end]).unwrap()
    }

    /// Sets the cursor to the position specified.
    ///
    /// This move is saturated on the tail.
    #[inline]
    pub fn seek(&mut self, index: usize) {
        let (steps, is_after) = match self.min_seek(index) {
            SeekFrom::Head(v) => {
                self.seek_head();
                (v, true)
            }
            SeekFrom::Tail(v) => {
                self.seek_tail();
                (v, false)
            }
            SeekFrom::Current(v) => (v.unsigned_abs(), v >= 0),
        };
        if is_after {
            for _ in 0..steps {
                self.seek_next();
            }
        } else {
            for _ in 0..steps {
                self.seek_prev();
            }
        }
    }

    /// Pushes a node to the head of the list.
    ///
    /// After the insert, the cursor will be located at that the head.
    #[inline]
    pub fn insert_head<'b>(&'b mut self, item: Pin<&'b Item<'_, T, M>>) -> DebugRefMut<'b, T> {
        self.list.insert_head(item.node());
        unsafe { item.borrow_data_unchecked() }
    }

    /// Pushes a node to the tail of the list.
    ///
    /// After the insert, the cursor will be located at the tail.
    #[inline]
    pub fn insert_tail<'b>(&'b mut self, item: Pin<&'b Item<'_, T, M>>) -> DebugRefMut<'b, T> {
        self.list.insert_tail(item.node());
        unsafe { item.borrow_data_unchecked() }
    }

    /// Pushes a node to the position specified.
    ///
    /// Inserts at the end of the list if `index` is greater than the length of the list.
    #[inline]
    pub fn insert(&mut self, index: usize, item: Pin<&Item<'_, T, M>>) {
        match index {
            0 => {
                self.insert_head(item);
                return;
            }
            n if n >= self.len() => {
                self.insert_tail(item);
                return;
            }
            _ => (),
        }

        let (steps, is_after) = match self.min_seek(index) {
            SeekFrom::Head(v) => {
                self.seek_head();
                (v, v != 0)
            }
            SeekFrom::Tail(v) => {
                self.seek_tail();
                (v, v != 0)
            }
            SeekFrom::Current(v) => (v.unsigned_abs(), v > 0),
        };

        let steps = steps.saturating_sub(1);
        if is_after {
            for _ in 0..steps {
                self.seek_next();
            }
            let cursor = self.get_cursor().unwrap();
            self.list.insert_after(cursor, item.node());
        } else {
            for _ in 0..steps {
                self.seek_prev();
            }
            let cursor = self.get_cursor().unwrap();
            self.list.insert_before(cursor, item.node());
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
            let mut node_ref = unsafe {
                let n = ItemData::from_node(item_node);
                n.data.borrow_mut()
            };
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
            let mut node_ref = unsafe {
                let n = ItemData::from_node(item_node);
                n.data.borrow_mut()
            };
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

    #[inline]
    fn inner_fold<B, C, F>(&mut self, init: C, mut f: F) -> ControlFlow<B, C>
    where
        F: FnMut(C, usize, &mut T) -> ControlFlow<B, C>,
    {
        if self.current.is_none() {
            self.current = self.list.head;
        }

        if self.is_empty() {
            return ControlFlow::Continue(init);
        }

        let mut acc = core::mem::MaybeUninit::new(init);
        loop {
            let index = self.index();
            {
                let Some(mut r) = self.get() else { unreachable!() };
                let next = f(unsafe { acc.assume_init_read() }, index, &mut *r)?;
                acc.write(next);
            }
            if self.is_tail() {
                break ControlFlow::Continue(unsafe { acc.assume_init_read() });
            }
            self.seek_next();
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

    #[inline]
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(usize, &mut T) -> bool,
    {
        if self.is_empty() {
            return;
        }

        if self.current.is_none() {
            self.seek_head();
        }

        loop {
            let index = self.index();
            let retain = {
                let Some(mut r) = self.get() else { unreachable!() };
                f(index, &mut *r)
            };
            if !retain {
                self.remove();
            }

            if self.is_tail() {
                break;
            }
            self.seek_next();
        }
    }

    #[inline]
    pub fn remove(&mut self) {
        let 
        self.list.remove(self);
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
