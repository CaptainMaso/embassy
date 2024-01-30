#[cfg(debug_assertions)]
use core::cell::RefCell;
#[cfg(not(debug_assertions))]
use core::cell::UnsafeCell;

use cfg_if::cfg_if;

#[cfg(debug_assertions)]
pub type Ref<'a, T: ?Sized> = core::cell::Ref<'a, T>;
#[cfg(debug_assertions)]
pub type RefMut<'a, T: ?Sized> = core::cell::RefMut<'a, T>;

#[cfg(not(debug_assertions))]
pub type Ref<'a, T: ?Sized> = &'a T;
#[cfg(not(debug_assertions))]
pub type RefMut<'a, T: ?Sized> = &'a mut T;

#[derive(Debug)]
pub struct DebugCell<T: ?Sized> {
    #[cfg(not(debug_assertions))]
    inner: UnsafeCell<T>,

    #[cfg(debug_assertions)]
    inner: RefCell<T>,
}

impl<T: ?Sized> DebugCell<T> {
    pub const fn new(data: T) -> Self
    where
        T: Sized,
    {
        cfg_if!(
            if #[cfg(debug_assertions)] {
                #[inline(always)]
                const fn f<T>(data : T) -> DebugCell<T> {
                    DebugCell { inner : RefCell::new(data) }
                }
            }
            else if #[cfg(not(debug_assertions))] {
                #[inline(always)]
                const fn f<T>(data : T) -> DebugCell<T> {
                    DebugCell { inner : UnsafeCell::new(data) }
                }
            }
        );

        f(data)
    }

    #[inline(always)]
    pub fn as_ptr(&self) -> *const T {
        cfg_if!(
            if #[cfg(debug_assertions)] {
                #[inline(always)]
                fn f<T : ?Sized>(rc : &RefCell<T>) -> *const T {
                    rc.as_ptr()
                }
            }
            else if #[cfg(not(debug_assertions))] {
                #[inline(always)]
                unsafe fn f<T : ?Sized>(rc : &UnsafeCell<T>) -> *const T {
                    rc.get()
                }
            }
        );

        f(&self.inner)
    }

    /// Copies the inner value
    ///
    /// SAFETY: Requires the caller to ensure no other unique references
    /// exist to the inner data.
    #[inline]
    pub unsafe fn get(&self) -> T
    where
        T: Copy,
    {
        *self.borrow()
    }

    /// Borrows the inner value
    ///
    /// SAFETY: Requires the caller to ensure no other unique references
    /// exist to the inner data.
    #[inline(always)]
    pub unsafe fn borrow(&self) -> Ref<'_, T> {
        cfg_if!(
            if #[cfg(debug_assertions)] {
                #[inline(always)]
                fn f<T : ?Sized>(rc : &RefCell<T>) -> core::cell::Ref<'_, T> {
                    rc.borrow()
                }
            }
            else if #[cfg(not(debug_assertions))] {
                #[inline(always)]
                unsafe fn f<T : ?Sized>(rc : &UnsafeCell<T>) -> &T {
                    rc.get().as_ref().unwrap()
                }
            }
        );

        f(&self.inner)
    }

    /// Mutably borrows the inner value
    ///
    /// SAFETY: Requires the caller to ensure no other shared references
    /// exist to the inner data
    #[inline(always)]
    pub unsafe fn borrow_mut(&self) -> RefMut<'_, T> {
        cfg_if!(
            if #[cfg(debug_assertions)] {
                #[inline(always)]
                fn f<T : ?Sized>(rc : &RefCell<T>) -> core::cell::RefMut<'_, T> {
                    rc.borrow_mut()
                }
            }
            else if #[cfg(not(debug_assertions))] {
                #[inline(always)]
                unsafe fn f<T : ?Sized>(rc : &UnsafeCell<T>) -> &mut T {
                    match rc.get().as_mut() {
                        Some(r) => r,
                        None => unreachable!()
                    }
                }
            }
        );

        f(&self.inner)
    }

    pub fn get_mut(&mut self) -> &mut T {
        cfg_if!(
            if #[cfg(debug_assertions)] {
                #[inline(always)]
                fn f<T : ?Sized>(rc : &mut RefCell<T>) -> &mut T {
                    rc.get_mut()
                }
            }
            else if #[cfg(not(debug_assertions))] {
                #[inline(always)]
                fn f<T : ?Sized>(rc : &mut UnsafeCell<T>) -> &mut T {
                    rc.get_mut()
                }
            }
        );

        f(&mut self.inner)
    }

    pub fn into_inner(self) -> T
    where
        T: Sized,
    {
        cfg_if!(
            if #[cfg(debug_assertions)] {
                #[inline(always)]
                fn f<T>(rc : RefCell<T>) -> T {
                    rc.into_inner()
                }
            }
            else if #[cfg(not(debug_assertions))] {
                #[inline(always)]
                fn f<T>(rc : UnsafeCell<T>) -> T {
                    rc.into_inner()
                }
            }
        );

        f(self.inner)
    }
}
