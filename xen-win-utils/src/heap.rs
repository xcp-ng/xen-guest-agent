use std::ops::{Deref, DerefMut};

use windows::Win32::{
    Foundation::{LocalFree, HGLOBAL, HLOCAL},
    System::Memory::{GlobalLock, GlobalSize, GlobalUnlock, LocalSize},
};

pub struct GlobalLockGuard<'a, T> {
    hmem: HGLOBAL,
    data: &'a mut [T],
}

impl<'a, T> GlobalLockGuard<'a, T> {
    pub fn lock(hmem: HGLOBAL) -> windows::core::Result<Self> {
        let p = unsafe { GlobalLock(hmem) } as *mut T;
        if p.is_null() {
            return Err(windows::core::Error::from_win32());
        }
        let len_bytes = unsafe { GlobalSize(hmem) };
        assert!(len_bytes > 0);
        assert!(len_bytes % align_of::<T>() == 0);
        let data = unsafe { core::slice::from_raw_parts_mut(p, len_bytes / size_of::<T>()) };
        Ok(Self { hmem, data })
    }

    pub fn get(&self) -> &[T] {
        self.data
    }

    pub fn get_mut(&mut self) -> &mut [T] {
        self.data
    }
}

impl<'a, T> Drop for GlobalLockGuard<'a, T> {
    fn drop(&mut self) {
        unsafe {
            let _ = GlobalUnlock(self.hmem);
        }
    }
}

/// This struct is only safe to use with LMEM_FIXED pointers!
pub struct LocalPointer<'a, T> {
    data: &'a mut [T],
}

impl<'a, T> LocalPointer<'a, T> {
    pub unsafe fn slice_from_raw_mut(p: *mut T) -> LocalPointer<'a, T> {
        assert!(!p.is_null());
        let len_bytes = LocalSize(HLOCAL(p.cast()));
        assert!(len_bytes > 0);
        assert!(len_bytes % align_of::<T>() == 0);
        LocalPointer {
            data: core::slice::from_raw_parts_mut(p, len_bytes / size_of::<T>()),
        }
    }

    pub unsafe fn from_raw_mut(p: *mut T) -> LocalPointer<'a, T> {
        assert!(!p.is_null());
        LocalPointer {
            data: core::slice::from_raw_parts_mut(p, 1),
        }
    }
}

impl<'a, T> Drop for LocalPointer<'a, T> {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.data.as_mut_ptr().cast())));
        }
    }
}

impl<'a, T> Deref for LocalPointer<'a, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a, T> DerefMut for LocalPointer<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}
