use core::pin::Pin;

use super::*;
use crate::blocking_mutex::raw::RawMutex;
use crate::debug_cell::{DebugCell, DebugRef, DebugRefMut};

#[pin_project::pin_project]
#[repr(C)]
pub(super) struct ItemData<T> {
    #[pin]
    node: Node,
    #[pin]
    pub(super) data: DebugCell<T>,
}

impl<T> ItemData<T> {
    /// Transmutes from a `&Node` to `ItemData`
    ///
    /// # Safety
    ///
    /// - Requires that the `&Node` be created from an `ItemData<T>`
    #[inline]
    pub unsafe fn from_node(node: Pin<&Node>) -> &Self {
        let ptr = (node.get_ref() as *const Node).cast::<Self>();

        ptr.as_ref().unwrap()
    }

    /// Gets a unique reference to the inner data
    ///
    /// # Safety
    ///
    /// - Requires that the caller has unique access to the inner list.
    #[inline]
    pub unsafe fn get_data(self: Pin<&Self>) -> DebugRefMut<'_, T> {
        self.project_ref().data.get_ref().borrow_mut()
    }

    pub const fn new(data: T) -> Self
    where
        T: Sized,
    {
        Self {
            node: Node::new(),
            data: DebugCell::new(data),
        }
    }
}

#[pin_project::pin_project(PinnedDrop)]
pub struct Item<'a, T, M: RawMutex> {
    list: &'a IntrusiveList<T, M>,
    #[pin]
    inner: ItemData<T>,
}

impl<'a, T, M: RawMutex> Item<'a, T, M> {
    pub fn node(self: Pin<&Self>) -> Pin<&Node> {
        self.project_ref().inner.project_ref().node
    }

    pub unsafe fn borrow_data_unchecked(self: Pin<&Self>) -> DebugRefMut<'_, T> {
        let data = self.project_ref().inner.project_ref().data.get_ref();
        unsafe { data.borrow_mut() }
    }

    pub fn try_borrow_data(self: Pin<&Self>) -> Option<DebugRef<'_, T>> {
        if self.as_ref().is_linked() {
            return None;
        }

        let proj = self.project_ref();
        let inner = proj.inner.project_ref();
        Some(unsafe { inner.data.get_ref().borrow() })
    }

    pub fn try_borrow_mut(self: Pin<&mut Self>) -> Option<DebugRefMut<'_, T>> {
        if self.as_ref().is_linked() {
            return None;
        }

        let proj = self.into_ref();
        let data = unsafe { proj.borrow_data_unchecked() };
        Some(data)
    }

    #[inline]
    pub fn with_cursor<O, F>(self: Pin<&mut Self>, f: F) -> O
    where
        F: FnOnce(&mut Cursor<'_, T, M>) -> O,
    {
        self.list.with_cursor(f)
    }

    #[inline]
    pub fn lock<O, F>(mut self: Pin<&mut Self>, f: F) -> O
    where
        F: FnOnce(&mut T) -> O,
    {
        let b = self.as_mut().try_borrow_mut();
        if let Some(mut t) = b {
            f(&mut *t)
        } else {
            drop(b);
            let proj = self.project();
            proj.list.with_cursor(|_c| {
                // Safety: If we have the cursor, we can safely get access to all data in the list.
                unsafe {
                    let inner = proj.inner.project();
                    let mut item_ref = inner.data.borrow_mut();
                    f(&mut *item_ref)
                }
            })
        }
    }

    /// Checks if the node is currently linked.
    ///
    /// If the node is unlinked, it will not be re-linked externally.
    ///
    /// If the node is linked, it may be un-linked externally.
    #[inline]
    pub fn is_linked(self: Pin<&Self>) -> bool {
        self.node().as_links().is_linked()
    }

    #[inline]
    pub fn remove(self: Pin<&Self>) {
        self.list.remove(self);
    }
}

unsafe impl<T, M: RawMutex> Send for Item<'_, T, M> {}
unsafe impl<T, M: RawMutex> Sync for Item<'_, T, M> {}

#[pin_project::pinned_drop]
impl<T, M: RawMutex> PinnedDrop for Item<'_, T, M> {
    fn drop(self: Pin<&mut Self>) {
        self.list.remove(self.as_ref());
    }
}
