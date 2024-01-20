/*
 * Copyright (c) 2017 Jorge Aparicio
 */

use core::iter::FusedIterator;

use super::*;

/// An iterator that moves out of a [`Deque`].
///
/// This struct is created by calling the `into_iter` method.
#[derive(Clone)]
pub struct IntoIter<T, const N: usize> {
    deque: Deque<T, N>,
}

impl<T, const N: usize> Iterator for IntoIter<T, N> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        self.deque.pop_front()
    }
}

impl<T, const N: usize> IntoIterator for Deque<T, N> {
    type Item = T;
    type IntoIter = IntoIter<T, N>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter { deque: self }
    }
}

/// An iterator over the elements of a [`Deque`].
///
/// This struct is created by calling the `iter` method.
#[derive(Clone)]
pub struct Iter<'a, T> {
    pub(super) inner: core::iter::Chain<core::slice::Iter<'a, T>, core::slice::Iter<'a, T>>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'a, T> DoubleEndedIterator for Iter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back()
    }
}

impl<'a, T> ExactSizeIterator for Iter<'a, T> {}
impl<'a, T> FusedIterator for Iter<'a, T> {}

/// An iterator over the elements of a [`Deque`].
///
/// This struct is created by calling the `iter` method.
pub struct IterMut<'a, T> {
    pub(super) inner: core::iter::Chain<core::slice::IterMut<'a, T>, core::slice::IterMut<'a, T>>,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'a, T> DoubleEndedIterator for IterMut<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back()
    }
}

impl<'a, T> ExactSizeIterator for IterMut<'a, T> {}
impl<'a, T> FusedIterator for IterMut<'a, T> {}

impl<'a, T, const N: usize> IntoIterator for &'a Deque<T, N> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a mut Deque<T, N> {
    type Item = &'a mut T;
    type IntoIter = IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
