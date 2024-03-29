//! Uniprocessor interior mutability primitives

use core::cell::{BorrowMutError, Ref, RefCell, RefMut};
use core::ops::{Deref, DerefMut};

/// Wrap a static data structure inside it so that we are
/// able to access it without any `unsafe`.
///
/// We should only use it in uniprocessor.
///
/// In order to get mutable reference of inner data, call
/// `exclusive_access`.
pub struct UPSafeCell<T> {
    /// inner data
    inner: RefCell<T>,
}

unsafe impl<T> Sync for UPSafeCell<T> {}

impl<T> UPSafeCell<T> {
    /// User is responsible to guarantee that inner struct is only used in
    /// uniprocessor.
    pub const unsafe fn new(value: T) -> Self {
        Self {
            inner: RefCell::new(value),
        }
    }
    /// Panic if the data has been borrowed.
    pub fn exclusive_access(&self) -> RefMut<'_, T> {
        // trace!("inner borrowed");
        self.inner.borrow_mut()
    }
    /// Panic if the data has been borrowed.
    pub fn try_exclusive_access(&self) -> Result<RefMut<'_, T>, BorrowMutError> {
        // trace!("inner borrowed");
        self.inner.try_borrow_mut()
    }
}
