use std::{
    io::{self, Read, Write},
    os::raw::c_void,
};

use windows::{
    core::{Owned, HSTRING},
    Win32::{
        Foundation::{ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE},
        Security::{SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR},
        Storage::FileSystem::{
            CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES,
            FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_SHARE_NONE, FILE_WRITE_DATA,
            OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
        },
        System::{
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
                PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
            },
            Threading::CreateEventW,
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        },
    },
};

#[repr(C)]
struct NamedPipeOverlapped<T>(OVERLAPPED, T);

struct ReadState {
    buffer: Box<[u8]>,
}

struct WriteState {
    buffer: Box<[u8]>,
}

struct ConnectState;

struct OperationState<T> {
    event: Option<Owned<HANDLE>>,
    state: Option<Box<NamedPipeOverlapped<T>>>,
}

impl<T> OperationState<T> {
    fn new(evented: bool) -> windows::core::Result<OperationState<T>> {
        Ok(OperationState {
            event: if evented {
                Some(unsafe { Owned::new(CreateEventW(None, false, false, None)?) })
            } else {
                None
            },
            state: None,
        })
    }

    unsafe fn get_event(&self) -> Option<HANDLE> {
        self.event.as_deref().map(|e| *e)
    }

    fn is_none(&self) -> bool {
        self.state.as_ref().is_none()
    }

    fn matches(&self, overlapped: *const OVERLAPPED) -> bool {
        self.state
            .as_deref()
            .is_some_and(|s| &s.0 as *const OVERLAPPED == overlapped)
    }

    fn take(&mut self) -> Option<Box<NamedPipeOverlapped<T>>> {
        self.state.take()
    }

    fn create(&self, t: T) -> Box<NamedPipeOverlapped<T>> {
        Box::new(NamedPipeOverlapped(
            OVERLAPPED {
                hEvent: unsafe { self.get_event().unwrap_or_default() },
                ..Default::default()
            },
            t,
        ))
    }

    fn put(&mut self, bx: Box<NamedPipeOverlapped<T>>) {
        let _ = self.state.insert(bx);
    }
}

pub enum NamedPipeResult {
    Read(Box<[u8]>),
    Written(u32),
    Connected,
}

pub struct NamedPipe {
    connect_state: OperationState<ConnectState>,
    read_state: OperationState<ReadState>,
    write_state: OperationState<WriteState>,
    pipe: Owned<HANDLE>,
    overlapped: bool,
    server: bool,
}

// not sure why HANDLEs are not Send by default, but whatever
unsafe impl Send for NamedPipe {}

impl NamedPipe {
    fn new(
        pipe: Owned<HANDLE>,
        overlapped: bool,
        server: bool,
        evented: bool,
    ) -> windows::core::Result<NamedPipe> {
        Ok(NamedPipe {
            connect_state: OperationState::<ConnectState>::new(evented)?,
            read_state: OperationState::<ReadState>::new(evented)?,
            write_state: OperationState::<WriteState>::new(evented)?,
            pipe,
            overlapped,
            server,
        })
    }

    pub fn create(
        name: &str,
        overlapped: bool,
        evented: bool,
        security_attributes: Option<(&SECURITY_DESCRIPTOR, bool)>,
    ) -> windows::core::Result<NamedPipe> {
        let wname = HSTRING::from(name);

        let dwopenmode = PIPE_ACCESS_DUPLEX
            | if overlapped {
                FILE_FLAG_OVERLAPPED
            } else {
                FILE_FLAGS_AND_ATTRIBUTES(0u32)
            };
        let sa = security_attributes.map(|(sd, inherit)| SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd as *const SECURITY_DESCRIPTOR as *mut c_void,
            bInheritHandle: inherit.into(),
        });
        let pipe = unsafe {
            Owned::new(CreateNamedPipeW(
                &wname,
                dwopenmode,
                PIPE_TYPE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                0,
                0,
                0,
                sa.as_ref().map(|x| x as *const SECURITY_ATTRIBUTES),
            ))
        };
        if pipe.is_invalid() {
            return Err(windows::core::Error::from_thread());
        }

        NamedPipe::new(pipe, overlapped, true, evented)
    }

    pub fn open(name: &str, overlapped: bool, evented: bool) -> windows::core::Result<NamedPipe> {
        let wname = HSTRING::from(name);

        // Don't let the server impersonate us.
        let mut flags = SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION;
        if overlapped {
            flags |= FILE_FLAG_OVERLAPPED;
        };
        let pipe = unsafe {
            Owned::new(CreateFileW(
                &wname,
                (FILE_GENERIC_READ | FILE_WRITE_DATA).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                flags,
                None,
            )?)
        };

        NamedPipe::new(pipe, overlapped, false, evented)
    }

    pub unsafe fn get_handle(&self) -> HANDLE {
        *self.pipe
    }

    pub unsafe fn get_read_event(&self) -> Option<HANDLE> {
        self.read_state.get_event()
    }

    pub unsafe fn get_write_event(&self) -> Option<HANDLE> {
        self.write_state.get_event()
    }

    pub unsafe fn get_connect_event(&self) -> Option<HANDLE> {
        self.connect_state.get_event()
    }

    pub fn is_server(&self) -> bool {
        self.server
    }

    fn get_overlapped_result(&self, overlapped: &OVERLAPPED) -> windows::core::Result<u32> {
        let mut count = 0u32;
        unsafe {
            GetOverlappedResult(*self.pipe, overlapped, &mut count, false)?;
        }
        Ok(count)
    }

    fn get_async_result<T>(result: windows::core::Result<T>) -> windows::core::Result<Option<T>> {
        match result {
            Ok(t) => Ok(Some(t)),
            Err(e) => {
                if e.code() == ERROR_IO_PENDING.into() {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }

    // returns true if data was read successfully, false if the read is async
    pub fn begin_read(&mut self, count: u32) -> windows::core::Result<bool> {
        assert!(self.overlapped);
        assert!(
            self.read_state.is_none(),
            "begin_read is not appropriate at this time"
        );

        let mut state = self.read_state.create(ReadState {
            buffer: vec![0u8; count as usize].into_boxed_slice(),
        });

        {
            let pstate = &mut state;
            unsafe {
                match Self::get_async_result(ReadFile(
                    *self.pipe,
                    Some(&mut pstate.1.buffer),
                    None,
                    Some(&mut pstate.0),
                ))? {
                    Some(_) => {
                        // after an early success, a completion packet is still queued
                        // must keep the state until it's completed
                        self.read_state.put(state);
                        Ok(true)
                    }
                    None => {
                        self.read_state.put(state);
                        Ok(false)
                    }
                }
            }
        }
    }

    unsafe fn complete_read(
        &mut self,
        state: Box<NamedPipeOverlapped<ReadState>>,
        result: windows::core::Result<u32>,
    ) -> windows::core::Result<Box<[u8]>> {
        // Low-level completion callback for IOCP and the like.
        match result {
            Ok(count) => {
                self.read_state.take();
                Ok(state.1.buffer[..(count as usize)]
                    .to_vec()
                    .into_boxed_slice())
            }
            Err(e) => {
                if e.code() == ERROR_IO_PENDING.into() {
                    panic!("read still pending");
                } else {
                    self.read_state.take();
                }
                Err(e)
            }
        }
    }

    // for use when the user is signaled by an event
    pub fn end_read_evented(&mut self) -> windows::core::Result<Box<[u8]>> {
        let state = self
            .read_state
            .take()
            .expect("end_read is not appropriate at this time");
        let result = self.get_overlapped_result(&state.0);
        unsafe { self.complete_read(state, result) }
    }

    pub fn cancel_read(&mut self) -> windows::core::Result<()> {
        if let Some(state) = self.read_state.take() {
            self.do_cancel_io(state)?;
        }
        Ok(())
    }

    pub fn begin_write(&mut self, data: &[u8]) -> windows::core::Result<bool> {
        assert!(self.overlapped);
        assert!(
            self.write_state.is_none(),
            "begin_write is not appropriate at this time"
        );

        let mut state = self.write_state.create(WriteState {
            buffer: data.to_vec().into_boxed_slice(),
        });

        {
            let pstate = &mut state;
            unsafe {
                match Self::get_async_result(WriteFile(
                    *self.pipe,
                    Some(&pstate.1.buffer),
                    None,
                    Some(&mut pstate.0),
                ))? {
                    Some(_) => {
                        self.write_state.put(state);
                        Ok(true)
                    }
                    None => {
                        self.write_state.put(state);
                        Ok(false)
                    }
                }
            }
        }
    }

    unsafe fn complete_write(
        &mut self,
        _state: Box<NamedPipeOverlapped<WriteState>>,
        result: windows::core::Result<u32>,
    ) -> windows::core::Result<u32> {
        match result {
            Ok(count) => {
                self.write_state.take();
                Ok(count)
            }
            Err(e) => {
                if e.code() == ERROR_IO_PENDING.into() {
                    panic!("write still pending");
                } else {
                    self.write_state.take();
                }
                Err(e)
            }
        }
    }

    pub fn end_write_evented(&mut self) -> windows::core::Result<u32> {
        let state = self
            .write_state
            .take()
            .expect("end_write is not appropriate at this time");
        let result = self.get_overlapped_result(&state.0);
        unsafe { self.complete_write(state, result) }
    }

    pub fn cancel_write(&mut self) -> windows::core::Result<()> {
        if let Some(state) = self.write_state.take() {
            self.do_cancel_io(state)?;
        }
        Ok(())
    }

    /// returns true if there's a client already connected
    ///
    /// if true, no need to call end_connect()
    pub fn begin_connect(&mut self) -> windows::core::Result<bool> {
        assert!(self.overlapped);
        assert!(self.server, "pipe is not a server");
        assert!(
            self.connect_state.is_none(),
            "begin_connect is not appropriate at this time"
        );

        let mut state = self.connect_state.create(ConnectState);

        let pstate = &mut state;
        unsafe {
            match ConnectNamedPipe(*self.pipe, Some(&mut pstate.0)) {
                Ok(_) => panic!("Unexpected ConnectNamedPipe result"),
                Err(e) => match e.code() {
                    hr if hr == ERROR_IO_PENDING.into() => {
                        self.connect_state.put(state);
                        Ok(false)
                    }
                    hr if hr == ERROR_PIPE_CONNECTED.into() => {
                        // TODO: ERROR_PIPE_CONNECTED should not generate a completion, but is it really the case?
                        self.connect_state.take();
                        Ok(true)
                    }
                    _ => {
                        log::error!("ConnectNamedPipe error {e}");
                        self.connect_state.take();
                        Err(e)
                    }
                },
            }
        }
    }

    unsafe fn complete_connect(
        &mut self,
        _state: Box<NamedPipeOverlapped<ConnectState>>,
        result: windows::core::Result<u32>,
    ) -> windows::core::Result<()> {
        match result {
            Ok(_) => {
                self.connect_state.take();
                Ok(())
            }
            Err(e) => {
                assert!(e.code() != ERROR_PIPE_CONNECTED.into());
                if e.code() == ERROR_IO_PENDING.into() {
                    panic!("connect still pending");
                } else {
                    self.connect_state.take();
                }
                Err(e)
            }
        }
    }

    pub fn end_connect_evented(&mut self) -> windows::core::Result<()> {
        let state = self
            .connect_state
            .take()
            .expect("end_connect is not appropriate at this time");
        let result = self.get_overlapped_result(&state.0);
        unsafe { self.complete_connect(state, result) }
    }

    pub fn cancel_connect(&mut self) -> windows::core::Result<()> {
        if let Some(state) = self.connect_state.take() {
            self.do_cancel_io(state)?;
        }
        Ok(())
    }

    pub unsafe fn complete_io(
        &mut self,
        overlapped: *const OVERLAPPED,
        result: windows::core::Result<u32>,
    ) -> windows::core::Result<NamedPipeResult> {
        // this function is not meant to be used in evented mode
        debug_assert!(self.get_read_event().is_none());
        if self.read_state.matches(overlapped) {
            let state = self.read_state.take().unwrap();
            let new_result = unsafe { self.complete_read(state, result)? };
            Ok(NamedPipeResult::Read(new_result))
        } else if self.write_state.matches(overlapped) {
            let state = self.write_state.take().unwrap();
            let new_result = unsafe { self.complete_write(state, result)? };
            Ok(NamedPipeResult::Written(new_result))
        } else if self.connect_state.matches(overlapped) {
            let state = self.connect_state.take().unwrap();
            unsafe { self.complete_connect(state, result)? };
            Ok(NamedPipeResult::Connected)
        } else {
            panic!("invalid overlapped pointer {overlapped:?}");
        }
    }

    fn cancel_all_io_silent(&mut self) {
        let _ = self
            .cancel_read()
            .inspect_err(|e| log::debug!("cancel_read {e}"));
        let _ = self
            .cancel_write()
            .inspect_err(|e| log::debug!("cancel_write {e}"));
        let _ = self
            .cancel_connect()
            .inspect_err(|e| log::debug!("cancel_connect {e}"));
    }

    pub fn disconnect(&mut self) -> windows::core::Result<()> {
        assert!(self.server);
        if unsafe { self.get_read_event().is_some() } {
            self.cancel_all_io_silent();
        }
        unsafe { DisconnectNamedPipe(*self.pipe)? };
        Ok(())
    }

    fn do_cancel_io<T>(&mut self, state: Box<NamedPipeOverlapped<T>>) -> windows::core::Result<()> {
        let mut count = 0u32;
        unsafe {
            CancelIoEx(*self.pipe, Some(&state.0))
                .and_then(|_| GetOverlappedResult(*self.pipe, &state.0, &mut count, true))
        }
    }
}

impl Read for NamedPipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        assert!(!self.overlapped);
        let mut read_count = 0u32;
        unsafe {
            ReadFile(*self.pipe, Some(buf), Some(&mut read_count), None)?;
        }
        Ok(read_count as usize)
    }
}

impl Write for NamedPipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        assert!(!self.overlapped);
        let mut write_count = 0u32;
        unsafe {
            WriteFile(*self.pipe, Some(buf), Some(&mut write_count), None)?;
        }
        Ok(write_count as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        unsafe {
            FlushFileBuffers(*self.pipe)?;
        }
        Ok(())
    }
}

impl Drop for NamedPipe {
    fn drop(&mut self) {
        if unsafe { self.get_read_event().is_some() } {
            self.cancel_all_io_silent();
        }
    }
}
