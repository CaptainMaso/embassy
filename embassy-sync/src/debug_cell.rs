use cfg_if::cfg_if;
#[cfg(debug_assertions)]
use core::cell::RefCell;
#[cfg(not(debug_assertions))]
use core::cell::UnsafeCell;

#[derive(Debug)]
pub struct DebugCell<T> {
    #[cfg(not(debug_assertions))]
    inner: UnsafeCell<T>,

    #[cfg(debug_assertions)]
    inner: RefCell<T>,
}

impl<T> DebugCell<T> {
    pub const fn new(data: T) -> Self {
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

    /// Borrows the inner value
    ///
    /// SAFETY: Requires the caller to ensure no other unique references
    /// exist to the inner data.
    #[inline(always)]
    pub unsafe fn borrow(&self) -> impl core::ops::Deref<Target = T> + '_ {
        cfg_if!(
            if #[cfg(debug_assertions)] {
                #[inline(always)]
                fn f<T>(rc : &RefCell<T>) -> impl core::ops::Deref<Target = T> + '_ {
                    rc.borrow()
                }
            }
            else if #[cfg(not(debug_assertions))] {
                #[inline(always)]
                unsafe fn f<T>(rc : &UnsafeCell<T>) -> &T {
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
    pub unsafe fn borrow_mut(&self) -> impl core::ops::DerefMut<Target = T> + '_ {
        cfg_if!(
            if #[cfg(debug_assertions)] {
                #[inline(always)]
                fn f<T>(rc : &RefCell<T>) -> impl core::ops::DerefMut<Target = T> + '_ {
                    rc.borrow_mut()
                }
            }
            else if #[cfg(not(debug_assertions))] {
                #[inline(always)]
                unsafe fn f<T>(rc : &UnsafeCell<T>) -> &mut T {
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
                fn f<T>(rc : &mut RefCell<T>) -> &mut T {
                    rc.get_mut()
                }
            }
            else if #[cfg(not(debug_assertions))] {
                #[inline(always)]
                fn f<T>(rc : &mut UnsafeCell<T>) -> &mut T {
                    rc.get_mut()
                }
            }
        );

        f(&mut self.inner)
    }
}
