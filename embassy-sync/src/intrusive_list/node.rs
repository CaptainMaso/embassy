use core::marker::PhantomPinned;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering as AtomicOrdering};

pub(super) struct AtomicNodePtr(AtomicPtr<Node>);

impl AtomicNodePtr {
    pub const fn new() -> Self {
        AtomicNodePtr(AtomicPtr::new(Self::UNLINKED_MARKER))
    }

    pub fn into_inner(self) -> NodeLink {
        NodeLink::from_node_ptr(self.0.into_inner())
    }

    #[inline]
    pub fn get(&self) -> NodeLink {
        let ptr = self.0.load(AtomicOrdering::SeqCst);
        if ptr.is_null() || ptr == Self::UNLINKED_MARKER {
            None
        } else {
            Some(NodePtr(ptr))
        }
    }

    #[inline]
    pub fn set_link(&self, ptr: NodePtr) -> NodeLink {
        let ptr = self.0.swap(ptr, AtomicOrdering::SeqCst);
        NodeLink::from_node_ptr(ptr)
    }

    #[inline]
    pub fn set_end(&self) -> NodeLink {
        let ptr = self.0.swap(NodeLink::END_MARKER, AtomicOrdering::SeqCst);
        NodeLink::from_node_ptr(ptr)
    }

    #[inline]
    pub fn clear(&self) -> NodeLink {
        let ptr = self.0.swap(NodeLink::UNLINKED_MARKER, AtomicOrdering::SeqCst);
        NodeLink::from_node_ptr(ptr)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum NodeLink {
    Ptr(NodePtr),
    End,
    Unlinked,
}

impl NodeLink {
    const UNLINKED_MARKER: *mut Node = 0 as *mut Node;
    const END_MARKER: *mut Node = 1 as *mut Node;

    #[inline(always)]
    fn from_node_ptr(ptr: *mut Node) -> Self {
        match ptr {
            Self::UNLINKED_MARKER => Self::Unlinked,
            Self::END_MARKER => Self::End,
            p => Self::Ptr(p),
        }
    }

    #[inline(always)]
    fn to_node_ptr(self) -> *mut Node {
        match self {
            Self::Unlinked => Self::UNLINKED_MARKER,
            Self::End => Self::END_MARKER,
            Self::Ptr(p) => p.0.as_ptr(),
        }
    }

    #[inline(always)]
    pub(super) fn expect_end(self) {
        match self {
            NodeLink::End => {
                #[cfg(debug_assertions)]
                panic!("Expected end marker");
                #[cfg(not(debug_assertions))]
                unreachable!()
            }
            _ => (),
        }
    }

    #[inline(always)]
    pub(super) fn expect_unlinked(self) {
        match self {
            NodeLink::Unlinked => {
                #[cfg(debug_assertions)]
                panic!("Expected unlinked");
                #[cfg(not(debug_assertions))]
                unreachable!()
            }
            _ => (),
        }
    }
}

pub(super) struct Node {
    _pin: PhantomPinned,
    next: AtomicNodePtr,
    prev: AtomicNodePtr,
}

impl Node {
    pub const fn new() -> Self {
        Self {
            _pin: PhantomPinned,
            prev: AtomicNodePtr::new(),
            next: AtomicNodePtr::new(),
        }
    }

    #[inline(always)]
    pub fn as_ptr(&self) -> NodePtr {
        NodePtr(core::ptr::NonNull::from(self))
    }

    #[inline]
    pub fn prev(&self) -> NodeLink {
        self.prev.get()
    }

    #[inline]
    pub fn next(&self) -> NodeLink {
        self.next.get()
    }

    /// Sets the previous link to the specified node.
    #[inline]
    pub fn update_prev(&self, ptr: &Self) -> NodeLink {
        self.prev.set(NodeLink::Ptr(ptr.as_ptr()))
    }

    /// Sets the next link to the specified node.
    #[inline]
    pub fn update_next(&self, ptr: &Self) -> NodeLink {
        self.next.set(NodeLink::Ptr(ptr.as_ptr()))
    }

    /// Sets the previous link to the end marker.
    #[inline]
    pub fn unlink_prev(&self) -> NodeLink {
        self.prev.set_end()
    }

    /// Sets the next link to the end marker.
    #[inline]
    pub fn unlink_next(&self) -> NodeLink {
        self.next.set_end()
    }

    #[inline]
    pub fn into_links(self) -> NodeLinks {
        let prev = self.prev.into_inner();
        let next = self.next.into_inner();

        match (prev, next) {
            (NodeLink::Unlinked, _) | (_, NodeLink::Unlinked) => NodeLinks::Unlinked,
            (NodeLink::Ptr(prev), NodeLink::Ptr(next)) => NodeLinks::Full { prev, next },
            (NodeLink::Ptr(prev), NodeLink::End) => NodeLinks::Tail { prev },
            (NodeLink::End, NodeLink::Ptr(next)) => NodeLinks::Head { next },
            (NodeLink::End, NodeLink::End) => NodeLinks::Single,
        }
    }

    #[inline]
    pub fn as_links(&self) -> NodeLinks {
        let ptr_read = unsafe { core::ptr::read(self) };
        ptr_read.into_links()
    }

    /// Unlinks this node from the node before and after it.
    ///
    /// Safety:
    ///
    ///  - Must be safe to create a shared reference to the node before
    ///  and after.
    #[inline]
    pub unsafe fn unlink(&self) {
        let links = self.as_links();

        self.prev.clear();
        self.next.clear();

        match links {
            NodeLinks::Unlinked => (),
            NodeLinks::Single => (),
            NodeLinks::Head { next } => next.get().unlink_prev(),
            NodeLinks::Tail { prev } => prev.get().unlink_next(),
            NodeLinks::Full { prev, next } => {
                let prev = prev.get();
                let next = next.get();
                prev.update_next(next);
                next.update_prev(prev);
            }
        }
    }

    /// Inserts this node into an empty list
    ///
    /// Safety:
    ///
    ///  - This node must have been previously unlinked
    #[inline]
    pub unsafe fn insert_empty(&self) {
        self.prev.set_end().expect_unlinked();
        self.next.set_end().expect_unlinked();
    }

    /// Inserts self as the head of the list, and updates the old head's links.
    ///
    /// Safety:
    ///
    ///  - This node must have been previously unlinked
    #[inline]
    pub unsafe fn insert_head(&self, old_head: &Self) {
        old_head.update_prev(self).expect_end();
        self.next.set_link(old_head.as_ptr()).expect_unlinked();
        self.prev.set_end().expect_unlinked();
    }

    /// Inserts self as the head of the list, and updates the old head's links.
    ///
    /// Safety:
    ///
    ///  - This node must have been previously unlinked
    #[inline]
    pub unsafe fn insert_tail(&self, old_tail: &Self) {
        old_tail.update_next(self);
        self.prev.set_link(old_tail).expect_unlinked();
        self.next.set_end().expect_unlinked();
    }

    /// Inserts self between two other nodes
    #[inline]
    pub unsafe fn insert_between(&self, prev: &Self, next: &Self) {
        self.update_prev(prev).expect_unlinked();
        prev.update_next(self);
        self.update_next(next).expect_unlinked();
        next.update_prev(self);
    }

    pub unsafe fn update_links(&self, links: NodeLinks) -> NodeLinks {
        core::mem::replace(&mut *self.get_links_mut(), links)
    }

    /// Clears the current nodes links and stitches the list
    /// together.
    #[inline]
    pub unsafe fn remove(&self) -> NodeLinks {
        let l = self.get_links_mut().clear();

        match &l {
            NodeLinks::Unlinked => (),
            NodeLinks::Single => (),
            NodeLinks::Head { next } => next.clear_prev(),
            NodeLinks::Tail { prev, .. } => prev.clear_next(),
            NodeLinks::Full { prev, next } => {
                next.set_prev(*prev);
                prev.set_next(*next);
            }
        }

        l
    }
}

#[derive(Debug, Default)]
pub(super) enum NodeLinks {
    #[default]
    Unlinked,
    Single,
    Head {
        next: NodePtr,
    },
    Tail {
        prev: NodePtr,
    },
    Full {
        prev: NodePtr,
        next: NodePtr,
    },
}

impl NodeLinks {
    #[inline]
    pub fn is_linked(&self) -> bool {
        match self {
            Self::Unlinked => false,
            _ => true,
        }
    }

    #[inline]
    pub fn next(&self) -> Option<NodePtr> {
        match self {
            NodeLinks::Head { next } | NodeLinks::Full { next, .. } => Some(*next),
            _ => None,
        }
    }

    #[inline]
    pub fn prev(&self) -> Option<NodePtr> {
        match self {
            NodeLinks::Tail { prev, .. } | NodeLinks::Full { prev, .. } => Some(*prev),
            _ => None,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NodePtr(NonNull<Node>);

impl NodePtr {
    #[inline(always)]
    pub unsafe fn get(&self) -> &Node {
        unsafe { self.0.as_ref() }
    }
}

pub struct NotLinked;
