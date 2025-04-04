use windows::{
    core::{Owned, HSTRING},
    Win32::{
        Foundation::{GetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE},
        System::Threading::CreateMutexW,
    },
};

pub struct NamedMutexGuard {
    _handle: Owned<HANDLE>,
}

impl NamedMutexGuard {
    pub fn new(
        name: Option<&str>,
        initial_owner: bool,
    ) -> windows::core::Result<Option<NamedMutexGuard>> {
        let wname = name.map_or(Default::default(), |x| HSTRING::from(x));
        let handle = unsafe { CreateMutexW(None, initial_owner, &wname)? };
        match unsafe { GetLastError() } {
            ERROR_SUCCESS => Ok(Some(NamedMutexGuard {
                _handle: unsafe { Owned::new(handle) },
            })),
            ERROR_ALREADY_EXISTS => Ok(None),
            e => panic!("unexpected error code {}", e.0),
        }
    }
}
