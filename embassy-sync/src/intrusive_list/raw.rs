use core::pin::Pin;

use super::*;

pub struct RawIntrusiveList {
    pub len: usize,
    pub head: Option<NodePtr>,
    pub tail: Option<NodePtr>,
}

impl RawIntrusiveList {
    pub const fn new() -> Self {
        Self {
            len: 0,
            head: None,
            tail: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        if self.head.is_some() && self.tail.is_some() {
            self.len
        } else {
            0
        }
    }

    #[inline(always)]
    pub fn head(&mut self) -> Option<Pin<&Node>> {
        self.head.as_ref().map(|n| unsafe { n.get() })
    }

    #[inline(always)]
    pub fn tail(&mut self) -> Option<Pin<&Node>> {
        self.tail.as_ref().map(|n| unsafe { n.get() })
    }

    /// Inserts a node at the head
    #[inline]
    pub fn insert_head(&mut self, new_head: Pin<&Node>) {
        unsafe { new_head.unlink() };
        if let Some(old_head) = self.head() {
            new_head.set_next(old_head).expect_unlinked();
            old_head.set_prev(new_head).expect_end();
            new_head.set_prev_end().expect_unlinked();
            self.head = Some(NodePtr::from_ref(new_head));
        } else {
            new_head.set_next_end();
            new_head.set_prev_end();
            let ptr = NodePtr::from_ref(new_head);
            self.head = Some(ptr);
            self.tail = Some(ptr);
        }
        self.len += 1;
    }

    /// Inserts a new node at the tail.
    #[inline]
    pub fn insert_tail(&mut self, new_tail: Pin<&Node>) {
        unsafe { new_tail.unlink() };
        if let Some(old_tail) = self.tail() {
            new_tail.set_prev(new_tail).expect_unlinked();
            old_tail.set_next(new_tail).expect_end();
            new_tail.set_prev_end().expect_unlinked();
            self.tail = Some(NodePtr::from_ref(new_tail));
        } else {
            new_tail.set_next_end();
            new_tail.set_prev_end();
            let ptr = NodePtr::from_ref(new_tail);
            self.head = Some(ptr);
            self.tail = Some(ptr);
        }
        self.len += 1;
    }

    /// Inserts self between two other nodes
    #[inline]
    pub fn insert_between(&mut self, prev: Pin<&Node>, new: Pin<&Node>, next: Pin<&Node>) {
        unsafe { new.unlink() };
        new.set_prev(prev).expect_unlinked();
        new.set_next(next).expect_unlinked();
        prev.set_next(new).expect_node(next);
        next.set_prev(new).expect_node(prev);
    }

    #[inline]
    pub fn insert_after(&mut self, cursor: Pin<&Node>, node: Pin<&Node>) {
        unsafe { node.unlink() };
        if let Some(next) = unsafe { cursor.next_ref() } {
            self.insert_between(cursor, node, next);
        } else {
            self.insert_tail(node);
        }
    }

    #[inline]
    pub fn insert_before(&mut self, cursor: Pin<&Node>, node: Pin<&Node>) {
        unsafe { node.unlink() };
        if let Some(prev) = unsafe { cursor.prev_ref() } {
            self.insert_between(prev, node, cursor);
        } else {
            self.insert_tail(node);
        }
    }

    pub(super) fn remove(&mut self, node: Pin<&Node>) {
        let links = node.as_links();
        unsafe {
            node.unlink();
        }
        match links {
            NodeLinks::Unlinked => return,
            NodeLinks::Single => {
                self.head = None;
                self.tail = None;
                self.len = 0;
            }
            NodeLinks::Head { next } => {
                self.head = Some(next);
            }
            NodeLinks::Tail { prev } => {
                self.tail = Some(prev);
            }
            NodeLinks::Full { prev, next } => {
                // Don't need to update any of our values, just unlink the removed node.
            }
        }
        self.len = self.len.saturating_sub(1);
    }
}
