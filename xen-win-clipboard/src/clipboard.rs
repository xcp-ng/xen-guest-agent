use std::marker::PhantomData;

use windows::Win32::{
    Foundation::{ERROR_BUFFER_OVERFLOW, HANDLE, HGLOBAL, HWND},
    System::{
        DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard, SetClipboardData},
        Memory::{GlobalAlloc, GMEM_MOVEABLE},
        Ole::{CF_UNICODETEXT, CLIPBOARD_FORMAT},
    },
};
use xen_win_utils::heap::GlobalLockGuard;

fn as_u8_slice(input: &[u16]) -> windows::core::Result<&[u8]> {
    let count = input.len().checked_mul(2).ok_or(ERROR_BUFFER_OVERFLOW)?;
    unsafe {
        Ok(core::slice::from_raw_parts(
            input.as_ptr() as *const u8,
            count,
        ))
    }
}

fn as_u16_box(input: &[u8]) -> Box<[u16]> {
    let count = input.len() / 2;
    let mut bx = vec![0u16; count];
    bx.copy_from_slice(unsafe { core::slice::from_raw_parts(input.as_ptr().cast(), count) });
    bx.into_boxed_slice()
}

pub(crate) struct Clipboard {
    _private: PhantomData<()>,
}

impl Clipboard {
    pub fn new(hwnd: HWND) -> windows::core::Result<Self> {
        unsafe {
            OpenClipboard(Some(hwnd))?;
        }
        Ok(Self {
            _private: PhantomData,
        })
    }

    fn get_raw(&self, format: CLIPBOARD_FORMAT) -> windows::core::Result<GlobalLockGuard<u8>> {
        let hcb = unsafe { GetClipboardData(format.0.into()) }?;
        let hmem = HGLOBAL(hcb.0);

        Ok(GlobalLockGuard::lock(hmem)?)
    }

    #[allow(dead_code)]
    pub fn get(&self, format: CLIPBOARD_FORMAT) -> windows::core::Result<Box<[u8]>> {
        let hmem_lock = self.get_raw(format)?;
        Ok(hmem_lock.get().to_vec().into_boxed_slice())
    }

    /// Null-terminated.
    pub fn get_wide_z(&self) -> windows::core::Result<Box<[u16]>> {
        let hmem_lock = self.get_raw(CF_UNICODETEXT)?;
        Ok(as_u16_box(hmem_lock.get()))
    }

    pub fn set(&self, format: CLIPBOARD_FORMAT, data: &[u8]) -> windows::core::Result<()> {
        // ownership transferred to the system
        let hmem = unsafe { GlobalAlloc(GMEM_MOVEABLE, data.len())? };
        {
            let mut hmem_lock = GlobalLockGuard::lock(hmem)?;
            hmem_lock.get_mut().copy_from_slice(data);
        }
        unsafe { SetClipboardData(format.0.into(), Some(HANDLE(hmem.0))) }?;
        Ok(())
    }

    /// Null-terminated.
    pub fn set_wide_z(&self, data_z: &[u16]) -> windows::core::Result<()> {
        self.set(CF_UNICODETEXT, as_u8_slice(data_z)?)
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}
