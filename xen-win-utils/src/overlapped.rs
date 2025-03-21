use windows::{
    Wdk::Storage::FileSystem::{
        FileReplaceCompletionInformation, NtSetInformationFile, FILE_COMPLETION_INFORMATION,
    },
    Win32::{
        Foundation::{
            HANDLE, WAIT_ABANDONED_0, WAIT_EVENT, WAIT_IO_COMPLETION, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        System::{Threading::INFINITE, IO::IO_STATUS_BLOCK},
        UI::WindowsAndMessaging::{
            MsgWaitForMultipleObjectsEx, MWMO_ALERTABLE, MWMO_INPUTAVAILABLE, MWMO_NONE,
            MWMO_WAITALL, QUEUE_STATUS_FLAGS,
        },
    },
};

const MAXIMUM_WAIT_OBJECTS: u32 = WAIT_IO_COMPLETION.0 - WAIT_ABANDONED_0.0;

pub enum WindowedWaitResult {
    Handle(u32),
    Input,
    Abandoned(u32),
    IoCompletion,
    Timeout,
}

pub fn windowed_wait(
    handles: Option<&[HANDLE]>,
    timeout_msec: u32,
    wake_mask: QUEUE_STATUS_FLAGS,
    alertable: bool,
    // level-triggered on window messages
    input_available: bool,
    wait_all: bool,
) -> windows::core::Result<WindowedWaitResult> {
    let mut flags = MWMO_NONE;
    if alertable {
        flags |= MWMO_ALERTABLE;
    }
    if input_available {
        flags |= MWMO_INPUTAVAILABLE;
    }
    if wait_all {
        flags |= MWMO_WAITALL;
    }

    let wait_count = handles.map(|h| h.len()).unwrap_or(0).try_into().unwrap();
    assert!(wait_count < MAXIMUM_WAIT_OBJECTS);
    let WAIT_EVENT(wait_result) =
        unsafe { MsgWaitForMultipleObjectsEx(handles, timeout_msec, wake_mask, flags) };

    if wait_result == wait_count {
        Ok(WindowedWaitResult::Input)
    } else if wait_result >= WAIT_OBJECT_0.0 && wait_result < WAIT_OBJECT_0.0 + wait_count {
        Ok(WindowedWaitResult::Handle(wait_result - WAIT_OBJECT_0.0))
    } else if wait_result >= WAIT_ABANDONED_0.0 && wait_result < WAIT_ABANDONED_0.0 + wait_count {
        Ok(WindowedWaitResult::Abandoned(
            wait_result - WAIT_ABANDONED_0.0,
        ))
    } else if wait_result == WAIT_IO_COMPLETION.0 {
        assert!(alertable);
        Ok(WindowedWaitResult::IoCompletion)
    } else if wait_result == WAIT_TIMEOUT.0 {
        assert!(timeout_msec != INFINITE);
        Ok(WindowedWaitResult::Timeout)
    } else {
        return Err(windows::core::Error::from_win32().into());
    }
}

pub unsafe fn clear_io_completion_port(handle: HANDLE) -> Result<(), windows::core::Error> {
    let mut iosb = IO_STATUS_BLOCK::default();
    let frci = FILE_COMPLETION_INFORMATION::default();
    NtSetInformationFile(
        handle,
        &mut iosb,
        (&frci as *const FILE_COMPLETION_INFORMATION).cast(),
        size_of::<FILE_COMPLETION_INFORMATION>() as u32,
        FileReplaceCompletionInformation,
    )
    .ok()
}
