use core::borrow::Borrow;
use core::pin::Pin;

use pin_project::pin_project;

use super::*;
use crate::blocking_mutex::raw::RawMutex;
use crate::debug_cell::DebugCell;

#[pin_project]
pub(super) struct Node<T: ?Sized> {
    links: DebugCell<NodeLinks<T>>,
    data: DebugCell<T>,
}

impl<T: ?Sized> Node<T> {
    pub const fn new(data: T) -> Self
    where
        T: Sized,
    {
        Self {
            data: DebugCell::new(data),
            links: DebugCell::new(NodeLinks::Unlinked),
        }
    }

    /// Gets a shared reference to the node data
    ///
    /// SAFETY: Assumes that the caller has a valid shared reference to the `RawIntrusiveList`
    pub unsafe fn get_data(&self) -> crate::debug_cell::Ref<'_, T> {
        self.data.borrow()
    }

    /// Gets a mutable reference to the node data
    ///
    /// SAFETY: Assumes that the caller has a valid unique reference to the `RawIntrusiveList`
    pub unsafe fn get_data_mut(&self) -> crate::debug_cell::RefMut<'_, T> {
        self.data.borrow_mut()
    }

    /// Gets a shared reference to the node data
    ///
    /// SAFETY: Assumes that the caller has a valid shared reference to the `RawIntrusiveList`
    unsafe fn get_links(&self) -> crate::debug_cell::Ref<'_, NodeLinks<T>> {
        self.links.borrow()
    }

    /// Gets a mutable reference to the node data
    ///
    /// SAFETY: Assumes that the caller has a valid unique reference to the `RawIntrusiveList`
    unsafe fn get_links_mut(&self) -> crate::debug_cell::RefMut<'_, NodeLinks<T>> {
        self.links.borrow_mut()
    }
}

#[derive(Debug, Default)]
pub enum NodeLinks<T: ?Sized> {
    #[default]
    Unlinked,
    Single,
    Head {
        next: NodeRef<T>,
    },
    Tail {
        prev: NodeRef<T>,
    },
    Full {
        prev: NodeRef<T>,
        next: NodeRef<T>,
    },
}
impl<T: ?Sized> NodeLinks<T> {
    #[inline]
    pub fn is_linked(&self) -> bool {
        match self {
            Self::Unlinked => false,
            _ => true,
        }
    }

    #[inline]
    pub fn next(&self) -> Option<NodeRef<T>> {
        match self {
            NodeLinks::Head { next } | NodeLinks::Full { next, .. } => Some(*next),
            _ => None,
        }
    }

    #[inline]
    pub fn set_prev(&mut self, node: NodeRef<T>) {
        match self {
            NodeLinks::Unlinked => panic!("Tried to set prev of an unlinked node"),
            NodeLinks::Single => *self = NodeLinks::Tail { prev: node },
            NodeLinks::Head { next } => {
                *self = NodeLinks::Full {
                    prev: node,
                    next: *next,
                }
            }
            NodeLinks::Tail { prev } | NodeLinks::Full { prev, .. } => {
                *prev = node;
            }
        }
    }

    #[inline]
    pub fn set_next(&mut self, node: NodeRef<T>) {
        match self {
            NodeLinks::Unlinked => panic!("Tried to set next of an unlinked node"),
            NodeLinks::Single => *self = NodeLinks::Head { next: node },
            NodeLinks::Tail { prev } => {
                *self = NodeLinks::Full {
                    prev: *prev,
                    next: node,
                }
            }
            NodeLinks::Head { next } | NodeLinks::Full { next, .. } => {
                *next = node;
            }
        }
    }

    #[inline]
    pub fn clear_prev(&mut self) {
        match self {
            NodeLinks::Unlinked => panic!("Tried to clear prev of an unlinked node"),
            NodeLinks::Single => panic!("Tried to clear prev of a single node"),
            NodeLinks::Head { next } => panic!("Tried to clear prev of the head node"),
            NodeLinks::Tail { prev } => *self = NodeLinks::Single,
            NodeLinks::Full { prev, next } => *self = NodeLinks::Head { next: *next },
        }
    }

    #[inline]
    pub fn clear_next(&mut self) {
        match self {
            NodeLinks::Unlinked => panic!("Tried to clear next of an unlinked node"),
            NodeLinks::Single => panic!("Tried to clear next of a single node"),
            NodeLinks::Tail { prev } => panic!("Tried to clear prev of the tail node"),
            NodeLinks::Head { next } => *self = NodeLinks::Single,
            NodeLinks::Full { prev, next } => *self = NodeLinks::Tail { prev: *prev },
        }
    }

    #[inline]
    pub fn prev(&self) -> Option<NodeRef<T>> {
        match self {
            NodeLinks::Tail { prev } | NodeLinks::Full { prev, .. } => Some(*prev),
            _ => None,
        }
    }

    #[inline]
    pub fn split(self) -> (Option<NodeRef<T>>, Option<NodeRef<T>>) {
        match self {
            NodeLinks::Full { prev, next } => (Some(prev), Some(next)),
            NodeLinks::Head { next } => (None, Some(next)),
            NodeLinks::Tail { prev } => (Some(prev), None),
            _ => (None, None),
        }
    }

    #[inline]
    pub fn clear(&mut self) -> Self {
        core::mem::take(self)
    }
}

#[derive(Debug)]
pub(super) struct NodeRef<T: ?Sized> {
    pub(super) ptr: *const Node<T>,
}

impl<T: ?Sized> Clone for NodeRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized> Copy for NodeRef<T> {}

impl<T: ?Sized> PartialEq for NodeRef<T> {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::addr_eq(self.ptr, other.ptr)
    }
}

impl<T> Eq for NodeRef<T> {}

impl<T: ?Sized> NodeRef<T> {
    /// Gets a reference to the `Node<T>`
    ///
    /// # Safety
    ///
    /// - The caller must ensure that there are no other mutable references to the `RawIntrusiveList`.
    /// - The caller must ensure that the particular node that is referenced is still registered to the list
    #[inline(always)]
    pub unsafe fn get_node_unchecked(&self) -> &Node<T> {
        self.ptr.as_ref().unwrap()
    }

    /// Gets a shared reference to the node data
    ///
    /// SAFETY: Assumes that the caller has a valid shared reference to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_data(&self) -> crate::debug_cell::Ref<'_, T> {
        self.get_node_unchecked().get_data()
    }

    /// Gets a mutable reference to the node data
    ///
    /// SAFETY: Assumes that the caller has a valid unique reference to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_data_mut(&self) -> crate::debug_cell::RefMut<'_, T> {
        self.get_node_unchecked().get_data_mut()
    }

    /// Gets a shared reference to the node data
    ///
    /// SAFETY: Assumes that the caller has a valid shared reference to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_links(&self) -> crate::debug_cell::Ref<'_, NodeLinks<T>> {
        self.get_node_unchecked().get_links()
    }

    /// Gets a mutable reference to the node data
    ///
    /// SAFETY: Assumes that the caller has a valid unique reference to the `RawIntrusiveList`
    #[inline(always)]
    pub unsafe fn get_links_mut(&self) -> crate::debug_cell::RefMut<'_, NodeLinks<T>> {
        self.get_node_unchecked().get_links_mut()
    }

    #[inline(always)]
    pub unsafe fn next(&self) -> Option<NodeRef<T>> {
        self.get_links().next()
    }

    #[inline(always)]
    pub unsafe fn prev(&self) -> Option<NodeRef<T>> {
        self.get_links().next()
    }

    #[inline(always)]
    pub unsafe fn set_next(&self, node: NodeRef<T>) {
        self.get_links_mut().set_next(node)
    }

    #[inline(always)]
    pub unsafe fn set_prev(&self, node: NodeRef<T>) {
        self.get_links_mut().set_next(node)
    }

    #[inline(always)]
    pub unsafe fn clear_next(&self) {
        self.get_links_mut().clear_next()
    }

    #[inline(always)]
    pub unsafe fn clear_prev(&self) {
        self.get_links_mut().clear_prev()
    }

    /// Inserts before this node
    ///
    /// # Panic
    ///
    /// This function panics if this node is unlinked.
    #[inline]
    pub unsafe fn insert_before(&self, node: NodeRef<T>) {
        node.set_next(*self);
        let mut links = self.get_links_mut();
        let links = &mut *links;
        match links {
            NodeLinks::Unlinked => panic!("Tried to insert before an unlinked node"),
            NodeLinks::Single => *links = NodeLinks::Head { next: node },
            NodeLinks::Head { next } => {
                *links = NodeLinks::Full {
                    prev: node,
                    next: *next,
                }
            }
            NodeLinks::Tail { prev } | NodeLinks::Full { prev, .. } => {
                let old_next = core::mem::replace(prev, node);
                old_next.set_prev(node);
                node.set_prev(old_next);
            }
        }
    }

    /// Inserts after this node
    ///
    /// # Safety
    ///
    /// Requires that the node have an
    ///
    /// # Panic
    ///
    /// This function panics if this node is unlinked.
    #[inline]
    pub unsafe fn insert_after(&self, node: NodeRef<T>) {
        node.set_prev(*self);
        let mut links = self.get_links_mut();
        let links = &mut *links;
        match links {
            NodeLinks::Unlinked => panic!("Tried to insert after an unlinked node"),
            NodeLinks::Single => *links = NodeLinks::Head { next: node },
            NodeLinks::Tail { prev } => {
                *links = NodeLinks::Full {
                    prev: *prev,
                    next: node,
                }
            }
            NodeLinks::Head { next } | NodeLinks::Full { next, .. } => {
                let old_next = core::mem::replace(next, node);
                old_next.set_prev(node);
                node.set_next(old_next);
            }
        }
    }

    pub unsafe fn update_links(&self, links: NodeLinks<T>) -> NodeLinks<T> {
        core::mem::replace(&mut *self.get_links_mut(), links)
    }

    /// Clears the current nodes links and stitches the list
    /// together.
    #[inline]
    pub unsafe fn remove(&self) -> NodeLinks<T> {
        let l = self.get_links_mut().clear();

        match &l {
            NodeLinks::Unlinked => (),
            NodeLinks::Single => (),
            NodeLinks::Head { next } => next.clear_prev(),
            NodeLinks::Tail { prev } => prev.clear_next(),
            NodeLinks::Full { prev, next } => {
                next.set_prev(*prev);
                prev.set_next(*next);
            }
        }

        l
    }
}
