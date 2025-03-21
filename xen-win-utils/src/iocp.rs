use std::{ffi::c_void, sync::LazyLock};

use windows::{
    core::{s, w, Owned, BOOL},
    Wdk::Foundation::OBJECT_ATTRIBUTES,
    Win32::{
        Foundation::{GENERIC_ALL, HANDLE, NTSTATUS, STATUS_SUCCESS},
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            IO::OVERLAPPED,
        },
    },
};

type NtCreateWaitCompletionPacket = unsafe extern "system" fn(
    WaitCompletionPacketHandle: *mut HANDLE,
    DesiredAccess: u32,
    ObjectAttributes: *const OBJECT_ATTRIBUTES,
) -> NTSTATUS;

type NtAssociateWaitCompletionPacket = unsafe extern "system" fn(
    WaitCompletionPacketHandle: HANDLE,
    IoCompletionHandle: HANDLE,
    TargetObjectHandle: HANDLE,
    KeyContext: *mut c_void,
    ApcContext: *mut c_void,
    IoStatus: NTSTATUS,
    IoStatusInformation: usize,
    AlreadySignaled: *mut BOOL,
) -> NTSTATUS;

type NtCancelWaitCompletionPacket = unsafe extern "system" fn(
    WaitCompletionPacketHandle: HANDLE,
    RemoveSignaledPacket: BOOL,
) -> NTSTATUS;

pub struct WaitCompletionHandle {
    handle: Owned<HANDLE>,
}

pub struct CompletionHelpers {
    nt_create_wait_completion_packet: NtCreateWaitCompletionPacket,
    nt_associate_wait_completion_packet: NtAssociateWaitCompletionPacket,
    nt_cancel_wait_completion_packet: NtCancelWaitCompletionPacket,
}

impl CompletionHelpers {
    pub fn new() -> windows::core::Result<CompletionHelpers> {
        unsafe {
            let ntdll = LoadLibraryW(w!("ntdll.dll"))?;
            let create_fn = GetProcAddress(ntdll, s!("NtCreateWaitCompletionPacket"))
                .ok_or(windows::core::Error::from_win32())?;
            let associate_fn = GetProcAddress(ntdll, s!("NtAssociateWaitCompletionPacket"))
                .ok_or(windows::core::Error::from_win32())?;
            let cancel_fn = GetProcAddress(ntdll, s!("NtCancelWaitCompletionPacket"))
                .ok_or(windows::core::Error::from_win32())?;
            Ok(CompletionHelpers {
                nt_create_wait_completion_packet: std::mem::transmute(create_fn),
                nt_associate_wait_completion_packet: std::mem::transmute(associate_fn),
                nt_cancel_wait_completion_packet: std::mem::transmute(cancel_fn),
            })
            // HACK: leak the ntdll.dll reference so that the static COMPLETION_HELPER is shareable
        }
    }

    pub unsafe fn create_wait(&self) -> windows::core::Result<WaitCompletionHandle> {
        unsafe {
            let mut handle = Owned::<HANDLE>::new(HANDLE::default());
            (self.nt_create_wait_completion_packet)(&mut *handle, GENERIC_ALL.0, std::ptr::null())
                .ok()?;
            Ok(WaitCompletionHandle { handle })
        }
    }

    pub unsafe fn associate_wait(
        &self,
        waiter: &mut WaitCompletionHandle,
        completion_port: HANDLE,
        event: HANDLE,
        key: usize,
        overlapped: *mut OVERLAPPED,
    ) -> windows::core::Result<bool> {
        unsafe {
            let mut already_signaled = BOOL::default();
            (self.nt_associate_wait_completion_packet)(
                *waiter.handle,
                completion_port,
                event,
                key as *mut c_void,
                overlapped.cast(),
                STATUS_SUCCESS,
                1,
                &mut already_signaled,
            )
            .ok()?;
            Ok(already_signaled.into())
        }
    }

    pub unsafe fn cancel_wait(
        &self,
        waiter: &mut WaitCompletionHandle,
        remove_signaled_packet: bool,
    ) -> windows::core::Result<()> {
        unsafe {
            (self.nt_cancel_wait_completion_packet)(*waiter.handle, remove_signaled_packet.into())
                .ok()?;
            Ok(())
        }
    }
}

pub static COMPLETION_HELPER: LazyLock<CompletionHelpers> =
    LazyLock::new(|| CompletionHelpers::new().expect("cannot resolve completion helper functions"));

pub struct EventCompletion {
    event: HANDLE,
    completion: WaitCompletionHandle,
    overlapped: Box<OVERLAPPED>,
}

impl EventCompletion {
    pub fn new(event: HANDLE) -> windows::core::Result<Self> {
        Ok(Self {
            event,
            completion: unsafe { COMPLETION_HELPER.create_wait()? },
            overlapped: Box::new(OVERLAPPED::default()),
        })
    }

    pub unsafe fn rearm(
        &mut self,
        completion_port: HANDLE,
        key: usize,
    ) -> windows::core::Result<()> {
        unsafe {
            let _ = COMPLETION_HELPER.associate_wait(
                &mut self.completion,
                completion_port,
                self.event,
                key,
                &mut *self.overlapped,
            )?;
        };
        Ok(())
    }
}
