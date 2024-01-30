use super::*;

pub struct RawIntrusiveList<T> {
    pub(super) len: usize,
    pub(super) links: Option<HeadTail<T>>,
}

pub(super) struct HeadTail<T> {
    pub head: NodeRef<T>,
    pub tail: NodeRef<T>,
}

impl<T> RawIntrusiveList<T> {
    pub const fn new() -> Self {
        Self { len: 0, links: None }
    }

    pub(super) fn cursor(&mut self) -> RawCursor<'_, T> {
        let head = self.get_head().map(|n| (0, n));
        RawCursor {
            list: self,
            current: head,
        }
    }

    #[inline(always)]
    pub(super) fn get_head(&self) -> Option<NodeRef<T>> {
        self.links.as_ref().map(|l| l.head)
    }

    /// Inserts a node at the head
    #[inline]
    pub(super) fn insert_head(&mut self, head: NodeRef<T>) {
        unsafe { head.update_links(NodeLinks::Single) };
        if let Some(l) = self.links.as_mut() {
            let old_head = core::mem::replace(&mut l.head, head);
            unsafe {
                old_head.insert_before(head);
            }
        } else {
            self.links = Some(HeadTail { head, tail: head });
        }
    }

    #[inline(always)]
    pub(super) fn get_tail(&self) -> Option<NodeRef<T>> {
        self.links.as_ref().map(|l| l.tail)
    }

    /// Inserts a new node at the tail.
    #[inline]
    pub(super) fn insert_tail(&mut self, tail: NodeRef<T>) {
        unsafe { tail.update_links(NodeLinks::Single) };
        if let Some(l) = self.links.as_mut() {
            let old_tail = core::mem::replace(&mut l.tail, tail);
            unsafe {
                old_tail.insert_after(tail);
            }
        } else {
            self.links = Some(HeadTail { head: tail, tail });
        }
    }

    pub(super) fn remove(&mut self, node: NodeRef<T>) {
        unsafe {
            match node.remove() {
                NodeLinks::Unlinked => {
                    self.len = 0;
                    self.links = None;

                    #[cfg(debug_assertions)]
                    panic!("Node was already unlinked but still in cursor");
                    #[cfg(not(debug_assertions))]
                    unreachable!()
                }
                NodeLinks::Single => {
                    self.links = None;
                    self.len = 0;
                }
                NodeLinks::Head { next } => {
                    let Some(l) = &mut self.links else {
                        self.len = 0;
                        self.links = None;
                        #[cfg(debug_assertions)]
                        panic!("Linked List HeadTail was empty");
                        #[cfg(not(debug_assertions))]
                        unreachable!()
                    };
                    l.head = next;
                }
                NodeLinks::Tail { prev } => {
                    let Some(l) = &mut self.links else {
                        self.len = 0;
                        self.links = None;
                        #[cfg(debug_assertions)]
                        panic!("Linked List HeadTail was empty");
                        #[cfg(not(debug_assertions))]
                        unreachable!()
                    };
                    l.tail = prev;
                }
                NodeLinks::Full { .. } => {}
            }
            self.len = self.len.saturating_sub(1);
        }
    }
}

pub(super) struct RawCursor<'a, T> {
    list: &'a mut RawIntrusiveList<T>,
    current: Option<(usize, NodeRef<T>)>,
}

impl<'a, T> RawCursor<'a, T> {
    pub fn len(&self) -> usize {
        self.list.len
    }

    pub fn is_empty(&self) -> bool {
        self.list.len == 0
    }

    pub fn idx(&self) -> Option<usize> {
        self.current.map(|(i, _)| i)
    }

    /// Moves the cursor to the next item in the list.
    /// Wraps to the head of the list if it reaches the end.
    pub fn next(&mut self) {
        let next = self
            .current
            .and_then(|(idx, n)| {
                let n = unsafe { n.next() };
                n.map(|n| (idx + 1, n))
            })
            .or_else(|| self.list.get_head().map(|n| (0, n)));

        self.current = next;
    }

    /// Moves the cursor to the previous item in the list.
    /// Wraps to the tail of the list if it reaches the end.
    pub fn prev(&mut self) {
        let prev = self
            .current
            .and_then(|(idx, n)| {
                let n = unsafe { n.prev() };
                n.map(|n| (idx - 1, n))
            })
            .or_else(|| self.list.get_tail().map(|n| (self.len() - 1, n)));

        self.current = prev;
    }

    /// Moves the cursor to the head of the list.
    pub fn head(&mut self) {
        self.current = self.list.get_head().map(|n| (0, n));
    }

    /// Moves the cursor to the tail of the list.
    pub fn tail(&mut self) {
        self.current = self.list.get_tail().map(|n| (self.len() - 1, n));
    }

    /// Gets a reference to the current item.
    ///
    /// Returns None if the list is empty
    pub fn get(&self) -> Option<crate::debug_cell::Ref<'_, T>> {
        self.current.as_ref().map(|(_, n)| unsafe { n.get_data() })
    }

    /// Gets a mutable reference to the current item.
    ///
    /// Returns None if the list is empty
    pub fn get_mut(&mut self) -> Option<crate::debug_cell::RefMut<'_, T>> {
        self.current.as_ref().map(|(_, n)| unsafe { n.get_data_mut() })
    }

    #[inline]
    pub fn insert_head(&mut self, node: NodeRef<T>) {
        self.list.insert_head(node);
        self.list.len += 1;
        if self.current.is_none() {
            self.current = Some((0, node));
        }
    }

    #[inline]
    pub fn insert_tail(&mut self, node: NodeRef<T>) {
        self.list.insert_tail(node);
        self.list.len += 1;
        if self.current.is_none() {
            self.current = Some((0, node));
        }
    }

    #[inline]
    pub fn insert_after(&mut self, node: NodeRef<T>) {
        if let Some((idx, cur)) = self.current {
            unsafe {
                cur.insert_after(node);
            }
            self.list.len += 1;
        } else {
            self.insert_tail(node);
        }
    }

    #[inline]
    pub fn insert_before(&mut self, node: NodeRef<T>) {
        if let Some((idx, cur)) = &mut self.current {
            unsafe {
                cur.insert_before(node);
            }
            self.list.len += 1;
        } else {
            self.insert_head(node);
        }
    }

    /// Removes the current item from the list, and moves the cursor to point at the previous item.
    ///
    /// If the item was the head of the list, the cursor is moved to the new head of the list.
    pub fn remove(&mut self) {
        if let Some((idx, n)) = self.current.take() {
            let next = unsafe { n.prev().map(|n| (idx - 1, n)).or_else(|| n.next().map(|n| (idx, n))) };
            self.list.remove(n);
            self.current = next;
        }
    }
}
