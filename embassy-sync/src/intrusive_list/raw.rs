use super::*;

pub struct RawIntrusiveList {
    pub(super) len: usize,
    pub(super) links: Option<HeadTail>,
}

pub(super) struct HeadTail {
    pub head: NodePtr,
    pub tail: NodePtr,
}

impl RawIntrusiveList {
    pub const fn new() -> Self {
        Self { len: 0, links: None }
    }

    pub(super) fn cursor(&mut self) -> RawCursor<'_> {
        let head = self.get_head().map(|n| (0, n));
        RawCursor {
            list: self,
            current: head,
        }
    }

    #[inline(always)]
    pub(super) fn get_head(&self) -> Option<NodePtr> {
        self.links.as_ref().map(|l| l.head)
    }

    /// Inserts a node at the head
    #[inline]
    pub(super) fn insert_head(&mut self, head: NodePtr) {
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
    pub(super) fn get_tail(&self) -> Option<NodePtr> {
        self.links.as_ref().map(|l| l.tail)
    }

    /// Inserts a new node at the tail.
    #[inline]
    pub(super) fn insert_tail(&mut self, tail: NodePtr) {
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

    pub(super) fn remove(&mut self, node: NodePtr) {
        unsafe {
            match node.remove() {
                NodeLinks::Unlinked => {
                    return;
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

pub(super) struct RawCursor<'a> {
    list: &'a mut RawIntrusiveList,
    current: Option<(usize, NodePtr)>,
}

impl<'a> RawCursor<'a> {
    pub fn current(&mut self) -> Option<&mut (usize, NodePtr)> {
        if self.current.is_none() {
            let head = self.list.get_head()?;
            Some(self.current.insert((0, head)))
        } else {
            Some(self.current.as_mut().unwrap())
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        if self.current.is_none() {
            return 0;
        }
        self.list.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.list.len == 0 || self.current.is_none()
    }

    #[inline]
    pub fn idx(&self) -> usize {
        self.current.map(|(i, _)| i).unwrap_or(0)
    }

    #[inline]
    pub fn is_head(&self) -> bool {
        self.current.map(|(idx, _)| idx == 0).unwrap_or(false)
    }

    #[inline]
    pub fn is_tail(&self) -> bool {
        self.current
            .map(|(idx, _)| idx == self.len().saturating_sub(1))
            .unwrap_or(false)
    }

    /// Moves the cursor to the next item in the list.
    /// Wraps to the head of the list if it reaches the end.
    #[inline]
    pub fn next(&mut self) {
        let next = self.current.and_then(|(idx, n)| {
            let n = unsafe { n.next() };
            n.map(|n| (idx + 1, n))
        });

        self.current = next;
    }

    /// Moves the cursor to the previous item in the list.
    /// Wraps to the tail of the list if it reaches the end.
    #[inline]
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
    #[inline]
    pub fn head(&mut self) {
        self.current = self.list.get_head().map(|n| (0, n));
    }

    /// Moves the cursor to the tail of the list.
    #[inline]
    pub fn tail(&mut self) {
        self.current = self.list.get_tail().map(|n| (self.len() - 1, n));
    }

    #[inline]
    pub fn insert_head(&mut self, node: NodePtr) {
        self.list.insert_head(node);
        self.list.len += 1;
        if self.current.is_none() {
            self.current = Some((0, node));
        }
    }

    #[inline]
    pub fn insert_tail(&mut self, node: NodePtr) {
        self.list.insert_tail(node);
        self.list.len += 1;
        if self.current.is_none() {
            self.current = Some((0, node));
        }
    }

    #[inline]
    pub fn insert_after(&mut self, node: NodePtr) {
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
    pub fn insert_before(&mut self, node: NodePtr) {
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
    #[inline]
    pub fn remove(&mut self) {
        if let Some((idx, n)) = self.current.take() {
            let next = unsafe { n.prev().map(|n| (idx - 1, n)).or_else(|| n.next().map(|n| (idx, n))) };
            self.list.remove(n);
            self.current = next;
        }
    }
}
