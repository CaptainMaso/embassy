use core::marker::PhantomPinned;
use core::pin::Pin;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering as AtomicOrdering};

pub(super) struct AtomicNodePtr(AtomicPtr<Node>);

impl AtomicNodePtr {
    pub const fn new() -> Self {
        AtomicNodePtr(AtomicPtr::new(NodeLink::UNLINKED_MARKER))
    }

    pub fn into_inner(self) -> NodeLink {
        NodeLink::from_node_ptr(self.0.into_inner())
    }

    #[inline]
    pub fn get(&self) -> NodeLink {
        let ptr = self.0.load(AtomicOrdering::SeqCst);
        NodeLink::from_node_ptr(ptr)
    }

    #[inline]
    pub fn set_link(&self, ptr: NodePtr) -> NodeLink {
        let ptr = self.0.swap(ptr.0.as_ptr(), AtomicOrdering::SeqCst);
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
            p => Self::Ptr(NodePtr(NonNull::new(p).unwrap())),
        }
    }

    #[inline]
    fn to_node_ptr(self) -> *mut Node {
        match self {
            Self::Unlinked => Self::UNLINKED_MARKER,
            Self::End => Self::END_MARKER,
            Self::Ptr(p) => p.0.as_ptr(),
        }
    }

    #[inline(always)]
    pub(super) fn expect_node(self, node: Pin<&Node>) {
        match self {
            NodeLink::Ptr(ptr) if ptr == NodePtr::from_ref(node) => (),
            _ => {
                #[cfg(debug_assertions)]
                panic!("Expected node to be linked to {:?}", node.as_ptr());
                #[cfg(not(debug_assertions))]
                unreachable!()
            }
        }
    }

    /// Converts to an option, expected the node to be linked
    ///
    /// # Safety
    ///
    /// - Expects the node to be linked.
    ///
    /// # Panic
    ///
    /// - Panics if the node was unlinked in debug mode.
    #[inline]
    pub fn expect_linked(self) -> Option<NodePtr> {
        match self {
            NodeLink::Ptr(ptr) => Some(ptr),
            NodeLink::End => None,
            NodeLink::Unlinked => {
                #[cfg(debug_assertions)]
                panic!("Expected node to be linked");
                #[cfg(not(debug_assertions))]
                unreachable!()
            }
        }
    }

    #[inline]
    pub(super) fn expect_end(self) {
        if let NodeLink::End = self {
            #[cfg(debug_assertions)]
            panic!("Expected end marker");
            #[cfg(not(debug_assertions))]
            unreachable!()
        }
    }

    #[inline(always)]
    pub(super) fn expect_unlinked(self) {
        match self {
            NodeLink::Unlinked => (),
            _ => {
                #[cfg(debug_assertions)]
                panic!("Expected unlinked");
                #[cfg(not(debug_assertions))]
                unreachable!()
            }
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
    pub fn as_ptr(self: Pin<&Self>) -> NodePtr {
        NodePtr::from_ref(self)
    }

    #[inline]
    pub fn prev(&self) -> NodeLink {
        self.prev.get()
    }

    #[inline]
    pub unsafe fn prev_ref(self: Pin<&Self>) -> Option<Pin<&Self>> {
        if let Some(prev) = self.prev().expect_linked() {
            Some(Pin::new_unchecked(prev.0.as_ref()))
        } else {
            None
        }
    }

    #[inline]
    pub fn next(&self) -> NodeLink {
        self.next.get()
    }

    #[inline]
    pub unsafe fn next_ref(self: Pin<&Self>) -> Option<Pin<&Self>> {
        if let Some(next) = self.next().expect_linked() {
            Some(Pin::new_unchecked(next.0.as_ref()))
        } else {
            None
        }
    }

    /// Sets the previous link to the specified node.
    #[inline]
    pub fn set_prev(&self, prev: Pin<&Self>) -> NodeLink {
        self.prev.set_link(NodePtr::from_ref(prev))
    }

    /// Sets the next link to the specified node.
    #[inline]
    pub fn set_next(&self, next: Pin<&Self>) -> NodeLink {
        self.next.set_link(NodePtr::from_ref(next))
    }

    /// Sets the previous link to the end marker.
    #[inline]
    pub fn set_prev_end(&self) -> NodeLink {
        self.prev.set_end()
    }

    /// Sets the next link to the end marker.
    #[inline]
    pub fn set_next_end(&self) -> NodeLink {
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

    /// Unlinks this node from the node before and after it,
    /// and stitches them together.
    ///
    /// Safety:
    ///
    ///  - Must be safe to create a shared reference to the node before
    ///  and after, if it is linked.
    ///  - If the node is unlinked, it is safe to call this at any time.
    #[inline]
    pub unsafe fn unlink(self: Pin<&Self>) {
        let links = self.as_links();

        self.prev.clear();
        self.next.clear();

        match links {
            NodeLinks::Unlinked => (),
            NodeLinks::Single => (),
            NodeLinks::Head { next } => {
                next.get().set_prev_end();
            }
            NodeLinks::Tail { prev } => {
                prev.get().set_next_end();
            }
            NodeLinks::Full { prev, next } => {
                let prev = prev.get();
                let next = next.get();
                prev.set_next(next);
                next.set_prev(prev);
            }
        }
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
    pub fn is_head(&self) -> bool {
        matches!(self, Self::Head { .. })
    }

    #[inline]
    pub fn is_tail(&self) -> bool {
        matches!(self, Self::Tail { .. })
    }

    #[inline]
    pub fn is_linked(&self) -> bool {
        !matches!(self, Self::Unlinked)
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
    pub unsafe fn get<'n>(&self) -> Pin<&'n Node> {
        unsafe { Pin::new_unchecked(self.0.as_ref()) }
    }

    pub fn from_ref(node: Pin<&Node>) -> Self {
        Self(NonNull::from(Pin::get_ref(node)))
    }
}

pub struct NotLinked;
