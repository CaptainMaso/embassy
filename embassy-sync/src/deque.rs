/*
 * Copyright (c) 2017 Jorge Aparicio
 */

use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::{fmt, ptr, slice};

pub mod iter;

use iter::*;

#[repr(C)]
pub struct Deque<T, const N: usize> {
    /// Front index. Always 0..=(N-1)
    start: usize,
    /// Back index. Always 0..=(N-1).
    len: usize,

    buffer: [MaybeUninit<T>; N],
}

impl<T, const N: usize> Deque<T, N> {
    const INIT: MaybeUninit<T> = MaybeUninit::uninit();

    /// Constructs a new, empty deque with a fixed capacity of `N`
    ///
    /// # Examples
    ///
    /// ```
    /// use heapless::Deque;
    ///
    /// // allocate the deque on the stack
    /// let mut x: Deque<u8, 16> = Deque::new();
    ///
    /// // allocate the deque in a static variable
    /// static mut X: Deque<u8, 16> = Deque::new();
    /// ```
    pub const fn new() -> Self {
        // Const assert N > 0
        //crate::sealed::greater_than_0::<N>();

        Self {
            buffer: [Self::INIT; N],
            start: 0,
            len: 0,
        }
    }

    fn increment(i: usize) -> usize {
        if i + 1 == N {
            0
        } else {
            i + 1
        }
    }

    fn decrement(i: usize) -> usize {
        if i == 0 {
            N - 1
        } else {
            i - 1
        }
    }
}

impl<T, const N: usize> core::ops::Deref for Deque<T, N> {
    type Target = DequeRef<T>;

    #[inline(always)]
    fn deref(&self) -> &DequeRef<T> {
        let p = self as *const _;
        let p = core::ptr::from_raw_parts::<DequeRef<T>>(p as *const (), N);
        unsafe { p.as_ref().unwrap() }
    }
}

impl<T, const N: usize> core::ops::DerefMut for Deque<T, N> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut DequeRef<T> {
        let p = self as *mut _;
        let p = core::ptr::from_raw_parts_mut::<DequeRef<T>>(p as *mut (), N);
        unsafe { p.as_mut().unwrap() }
    }
}

#[repr(C)]
pub struct DequeRef<T> {
    /// Front index. Always 0..=(N-1)
    start: usize,
    /// Back index. Always 0..=(N-1).
    len: usize,

    buffer: [MaybeUninit<T>],
}

impl<T> DequeRef<T> {
    /// Returns the maximum number of elements the deque can hold.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Returns the number of elements currently in the deque.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    const fn end(&self) -> usize {
        (self.start + self.len) % self.capacity()
    }

    const fn prev_idx(&self, idx: usize) -> usize {
        match idx.checked_sub(1) {
            Some(v) => v,
            None => self.capacity() - 1,
        }
    }

    const fn next_idx(&self, idx: usize) -> usize {
        (idx + 1) % self.capacity()
    }

    /// Clears the deque, removing all values.
    #[inline]
    pub fn clear(&mut self) {
        // safety: we're immediately setting a consistent empty state.
        unsafe { self.drop_contents() }
        self.start = 0;
        self.len = 0;
    }

    /// Drop all items in the `Deque`, leaving the state `back/front/full` unmodified.
    ///
    /// safety: leaves the `Deque` in an inconsistent state, so can cause duplicate drops.
    unsafe fn drop_contents(&mut self) {
        // We drop each element used in the deque by turning into a &mut[T]
        let (a, b) = self.as_mut_slices();
        ptr::drop_in_place(a);
        ptr::drop_in_place(b);
    }

    /// Returns whether the deque is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns whether the deque is full (i.e. if `len() == capacity()`.
    pub fn is_full(&self) -> bool {
        self.len == self.capacity()
    }

    /// Returns a pair of slices which contain, in order, the contents of the `Deque`.
    pub fn as_slices(&self) -> (&[T], &[T]) {
        let ptr = self.buffer.as_ptr();
        // NOTE(unsafe) avoid bound checks in the slicing operation
        unsafe {
            if self.is_empty() {
                (&[], &[])
            } else {
                let end = self.end();

                if self.start >= end {
                    (
                        slice::from_raw_parts(ptr.add(self.start) as *mut T, self.capacity() - self.start),
                        slice::from_raw_parts(ptr as *mut T, end),
                    )
                } else {
                    (slice::from_raw_parts(ptr.add(self.start) as *mut T, self.len), &[])
                }
            }
        }
    }

    /// Returns a pair of mutable slices which contain, in order, the contents of the `Deque`.
    pub fn as_mut_slices(&mut self) -> (&mut [T], &mut [T]) {
        let ptr = self.buffer.as_mut_ptr();

        // NOTE(unsafe) avoid bound checks in the slicing operation
        unsafe {
            if self.is_empty() {
                (&mut [], &mut [])
            } else {
                let end = self.end();

                if self.start >= end {
                    (
                        slice::from_raw_parts_mut(ptr.add(self.start) as *mut T, self.capacity() - self.start),
                        slice::from_raw_parts_mut(ptr as *mut T, end),
                    )
                } else {
                    (
                        slice::from_raw_parts_mut(ptr.add(self.start) as *mut T, self.len),
                        &mut [],
                    )
                }
            }
        }
    }

    /// Provides a reference to the front element, or None if the `Deque` is empty.
    pub fn front(&self) -> Option<&T> {
        if self.is_empty() {
            None
        } else {
            Some(unsafe { &*self.buffer.get_unchecked(self.start).as_ptr() })
        }
    }

    /// Provides a mutable reference to the front element, or None if the `Deque` is empty.
    pub fn front_mut(&mut self) -> Option<&mut T> {
        if self.is_empty() {
            None
        } else {
            Some(unsafe { &mut *self.buffer.get_unchecked_mut(self.start).as_mut_ptr() })
        }
    }

    /// Provides a reference to the back element, or None if the `Deque` is empty.
    pub fn back(&self) -> Option<&T> {
        if self.is_empty() {
            None
        } else {
            let index = self.prev_idx(self.end());
            Some(unsafe { &*self.buffer.get_unchecked(index).as_ptr() })
        }
    }

    /// Provides a mutable reference to the back element, or None if the `Deque` is empty.
    pub fn back_mut(&mut self) -> Option<&mut T> {
        if self.is_empty() {
            None
        } else {
            let index = self.prev_idx(self.end());
            Some(unsafe { &mut *self.buffer.get_unchecked_mut(index).as_mut_ptr() })
        }
    }

    /// Removes the item from the front of the deque and returns it, or `None` if it's empty
    pub fn pop_front(&mut self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            Some(unsafe { self.pop_front_unchecked() })
        }
    }

    /// Removes the item from the back of the deque and returns it, or `None` if it's empty
    pub fn pop_back(&mut self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            Some(unsafe { self.pop_back_unchecked() })
        }
    }

    /// Appends an `item` to the front of the deque
    ///
    /// Returns back the `item` if the deque is full
    pub fn push_front(&mut self, item: T) -> Result<(), T> {
        if self.is_full() {
            Err(item)
        } else {
            unsafe { self.push_front_unchecked(item) }
            Ok(())
        }
    }

    /// Appends an `item` to the back of the deque
    ///
    /// Returns back the `item` if the deque is full
    pub fn push_back(&mut self, item: T) -> Result<(), T> {
        if self.is_full() {
            Err(item)
        } else {
            unsafe { self.push_back_unchecked(item) }
            Ok(())
        }
    }

    /// Removes an item from the front of the deque and returns it, without checking that the deque
    /// is not empty
    ///
    /// # Safety
    ///
    /// It's undefined behavior to call this on an empty deque
    pub unsafe fn pop_front_unchecked(&mut self) -> T {
        debug_assert!(!self.is_empty());

        let index = self.start;
        self.start = self.next_idx(self.start);
        self.len -= 1;
        self.buffer.get_unchecked_mut(index).as_ptr().read()
    }

    /// Removes an item from the back of the deque and returns it, without checking that the deque
    /// is not empty
    ///
    /// # Safety
    ///
    /// It's undefined behavior to call this on an empty deque
    pub unsafe fn pop_back_unchecked(&mut self) -> T {
        debug_assert!(!self.is_empty());

        let index = self.prev_idx(self.end());
        self.len -= 1;
        self.buffer.get_unchecked_mut(index).as_ptr().read()
    }

    /// Appends an `item` to the front of the deque
    ///
    /// # Safety
    ///
    /// This assumes the deque is not full.
    pub unsafe fn push_front_unchecked(&mut self, item: T) {
        debug_assert!(!self.is_full());

        let index = self.prev_idx(self.start);
        // NOTE: the memory slot that we are about to write to is uninitialized. We assign
        // a `MaybeUninit` to avoid running `T`'s destructor on the uninitialized memory
        *self.buffer.get_unchecked_mut(index) = MaybeUninit::new(item);
        self.start = index;
        self.len += 1;
    }

    /// Appends an `item` to the back of the deque
    ///
    /// # Safety
    ///
    /// This assumes the deque is not full.
    pub unsafe fn push_back_unchecked(&mut self, item: T) {
        debug_assert!(!self.is_full());

        // NOTE: the memory slot that we are about to write to is uninitialized. We assign
        // a `MaybeUninit` to avoid running `T`'s destructor on the uninitialized memory
        let index = self.end();
        *self.buffer.get_unchecked_mut(index) = MaybeUninit::new(item);
        self.len += 1;
    }

    /// Returns an iterator over the deque.
    pub fn iter(&self) -> Iter<'_, T> {
        let (a, b) = self.as_slices();
        Iter {
            inner: a.into_iter().chain(b.into_iter()),
        }
    }

    /// Returns an iterator that allows modifying each value.
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        let (a, b) = self.as_mut_slices();
        IterMut {
            inner: a.into_iter().chain(b.into_iter()),
        }
    }
}

impl<T, const N: usize> Default for Deque<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Drop for Deque<T, N> {
    fn drop(&mut self) {
        // safety: `self` is left in an inconsistent state but it doesn't matter since
        // it's getting dropped. Nothing should be able to observe `self` after drop.
        unsafe { self.drop_contents() }
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for Deque<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self).finish()
    }
}

impl<T, const N: usize> Clone for Deque<T, N>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        let mut res = Deque::new();
        for i in self {
            // safety: the original and new deques have the same capacity, so it can
            // not become full.
            unsafe { res.push_back_unchecked(i.clone()) }
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::Deque;

    #[test]
    fn static_new() {
        static mut _V: Deque<i32, 4> = Deque::new();
    }

    #[test]
    fn stack_new() {
        let mut _v: Deque<i32, 4> = Deque::new();
    }

    // #[test]
    // fn drop() {
    //     crate::test_helper::droppable!();

    //     {
    //         let mut v: Deque<Droppable, 2> = Deque::new();
    //         v.push_back(Droppable::new()).ok().unwrap();
    //         v.push_back(Droppable::new()).ok().unwrap();
    //         v.pop_front().unwrap();
    //     }

    //     assert_eq!(Droppable::count(), 0);

    //     {
    //         let mut v: Deque<Droppable, 2> = Deque::new();
    //         v.push_back(Droppable::new()).ok().unwrap();
    //         v.push_back(Droppable::new()).ok().unwrap();
    //     }

    //     assert_eq!(Droppable::count(), 0);
    //     {
    //         let mut v: Deque<Droppable, 2> = Deque::new();
    //         v.push_front(Droppable::new()).ok().unwrap();
    //         v.push_front(Droppable::new()).ok().unwrap();
    //     }

    //     assert_eq!(Droppable::count(), 0);
    // }

    #[test]
    fn full() {
        let mut v: Deque<i32, 4> = Deque::new();

        v.push_back(0).unwrap();
        v.push_front(1).unwrap();
        v.push_back(2).unwrap();
        v.push_back(3).unwrap();

        assert!(v.push_front(4).is_err());
        assert!(v.push_back(4).is_err());
        assert!(v.is_full());
    }

    #[test]
    fn empty() {
        let mut v: Deque<i32, 4> = Deque::new();
        assert!(v.is_empty());

        v.push_back(0).unwrap();
        assert!(!v.is_empty());

        v.push_front(1).unwrap();
        assert!(!v.is_empty());

        v.pop_front().unwrap();
        v.pop_front().unwrap();

        assert!(v.pop_front().is_none());
        assert!(v.pop_back().is_none());
        assert!(v.is_empty());
    }

    #[test]
    fn front_back() {
        let mut v: Deque<i32, 4> = Deque::new();
        assert_eq!(v.front(), None);
        assert_eq!(v.front_mut(), None);
        assert_eq!(v.back(), None);
        assert_eq!(v.back_mut(), None);

        v.push_back(4).unwrap();
        assert_eq!(v.front(), Some(&4));
        assert_eq!(v.front_mut(), Some(&mut 4));
        assert_eq!(v.back(), Some(&4));
        assert_eq!(v.back_mut(), Some(&mut 4));

        v.push_front(3).unwrap();
        assert_eq!(v.front(), Some(&3));
        assert_eq!(v.front_mut(), Some(&mut 3));
        assert_eq!(v.back(), Some(&4));
        assert_eq!(v.back_mut(), Some(&mut 4));

        v.pop_back().unwrap();
        assert_eq!(v.front(), Some(&3));
        assert_eq!(v.front_mut(), Some(&mut 3));
        assert_eq!(v.back(), Some(&3));
        assert_eq!(v.back_mut(), Some(&mut 3));

        v.pop_front().unwrap();
        assert_eq!(v.front(), None);
        assert_eq!(v.front_mut(), None);
        assert_eq!(v.back(), None);
        assert_eq!(v.back_mut(), None);
    }

    #[test]
    fn iter() {
        let mut v: Deque<i32, 4> = Deque::new();

        v.push_back(0).unwrap();
        v.push_back(1).unwrap();
        v.push_front(2).unwrap();
        v.push_front(3).unwrap();
        v.pop_back().unwrap();
        v.push_front(4).unwrap();

        dbg!(&v);

        let mut items = v.iter();

        assert_eq!(items.next(), Some(&4));
        assert_eq!(items.next(), Some(&3));
        assert_eq!(items.next(), Some(&2));
        assert_eq!(items.next(), Some(&0));
        assert_eq!(items.next(), None);
    }

    #[test]
    fn iter_mut() {
        let mut v: Deque<i32, 4> = Deque::new();

        v.push_back(0).unwrap();
        v.push_back(1).unwrap();
        v.push_front(2).unwrap();
        v.push_front(3).unwrap();
        v.pop_back().unwrap();
        v.push_front(4).unwrap();

        let mut items = v.iter_mut();

        assert_eq!(items.next(), Some(&mut 4));
        assert_eq!(items.next(), Some(&mut 3));
        assert_eq!(items.next(), Some(&mut 2));
        assert_eq!(items.next(), Some(&mut 0));
        assert_eq!(items.next(), None);
    }

    #[test]
    fn iter_move() {
        let mut v: Deque<i32, 4> = Deque::new();
        v.push_back(0).unwrap();
        v.push_back(1).unwrap();
        v.push_back(2).unwrap();
        v.push_back(3).unwrap();

        let mut items = v.into_iter();

        assert_eq!(items.next(), Some(0));
        assert_eq!(items.next(), Some(1));
        assert_eq!(items.next(), Some(2));
        assert_eq!(items.next(), Some(3));
        assert_eq!(items.next(), None);
    }

    // #[test]
    // fn iter_move_drop() {
    //     crate::droppable!();

    //     {
    //         let mut deque: Deque<Droppable, 2> = Deque::new();
    //         deque.push_back(Droppable::new()).ok().unwrap();
    //         deque.push_back(Droppable::new()).ok().unwrap();
    //         let mut items = deque.into_iter();
    //         // Move all
    //         let _ = items.next();
    //         let _ = items.next();
    //     }

    //     assert_eq!(Droppable::count(), 0);

    //     {
    //         let mut deque: Deque<Droppable, 2> = Deque::new();
    //         deque.push_back(Droppable::new()).ok().unwrap();
    //         deque.push_back(Droppable::new()).ok().unwrap();
    //         let _items = deque.into_iter();
    //         // Move none
    //     }

    //     assert_eq!(Droppable::count(), 0);

    //     {
    //         let mut deque: Deque<Droppable, 2> = Deque::new();
    //         deque.push_back(Droppable::new()).ok().unwrap();
    //         deque.push_back(Droppable::new()).ok().unwrap();
    //         let mut items = deque.into_iter();
    //         let _ = items.next(); // Move partly
    //     }

    //     assert_eq!(Droppable::count(), 0);
    // }

    #[test]
    fn push_and_pop() {
        let mut q: Deque<i32, 4> = Deque::new();
        assert_eq!(q.len(), 0);

        assert_eq!(q.pop_front(), None);
        assert_eq!(q.pop_back(), None);
        assert_eq!(q.len(), 0);

        q.push_back(0).unwrap();
        assert_eq!(q.len(), 1);

        assert_eq!(q.pop_back(), Some(0));
        assert_eq!(q.len(), 0);

        q.push_back(0).unwrap();
        q.push_back(1).unwrap();
        q.push_front(2).unwrap();
        q.push_front(3).unwrap();
        assert_eq!(q.len(), 4);

        // deque contains: 3 2 0 1
        assert_eq!(q.pop_front(), Some(3));
        assert_eq!(q.len(), 3);
        assert_eq!(q.pop_front(), Some(2));
        assert_eq!(q.len(), 2);
        assert_eq!(q.pop_back(), Some(1));
        assert_eq!(q.len(), 1);
        assert_eq!(q.pop_front(), Some(0));
        assert_eq!(q.len(), 0);

        // deque is now empty
        assert_eq!(q.pop_front(), None);
        assert_eq!(q.pop_back(), None);
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn as_slices() {
        let mut q: Deque<i32, 4> = Deque::new();
        assert_eq!(q.len(), 0);

        q.push_back(0).unwrap();
        q.push_back(1).unwrap();
        q.push_back(2).unwrap();
        q.push_back(3).unwrap();
        assert_eq!(q.as_slices(), (&[0, 1, 2, 3][..], &[][..]));

        q.pop_front().unwrap();
        assert_eq!(q.as_slices(), (&[1, 2, 3][..], &[][..]));

        q.push_back(4).unwrap();
        assert_eq!(q.as_slices(), (&[1, 2, 3][..], &[4][..]));
    }

    #[test]
    fn clear() {
        let mut q: Deque<i32, 4> = Deque::new();
        assert_eq!(q.len(), 0);

        q.push_back(0).unwrap();
        q.push_back(1).unwrap();
        q.push_back(2).unwrap();
        q.push_back(3).unwrap();
        assert_eq!(q.len(), 4);

        q.clear();
        assert_eq!(q.len(), 0);

        q.push_back(0).unwrap();
        assert_eq!(q.len(), 1);
    }
}
