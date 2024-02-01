use core::marker::PhantomData;
use core::ops::ControlFlow;
use core::pin::Pin;

use self::raw::RawCursor;
use super::*;
use crate::blocking_mutex::raw::{ConstRawMutex, RawMutex};
use crate::blocking_mutex::Mutex;
use crate::debug_cell::{DebugCell, DebugRef, DebugRefMut};

pub struct IntrusiveList<T, M: RawMutex> {
    inner: Mutex<M, DebugCell<RawIntrusiveList>>,
    _data: PhantomData<T>,
}

unsafe impl<T, M: RawMutex> Sync for IntrusiveList<T, M> {}
unsafe impl<T, M: RawMutex> Send for IntrusiveList<T, M> {}

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

    pub const fn new_with(mutex: M) -> Self {
        Self {
            inner: Mutex::new_with(DebugCell::new(RawIntrusiveList::new()), mutex),
            _data: PhantomData,
        }
    }

    pub const fn new_store(&self, item: T) -> Item<'_, T, M> {
        Item {
            inner: ItemData::new(item),
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
                _m: PhantomData,
                _t: PhantomData,
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
