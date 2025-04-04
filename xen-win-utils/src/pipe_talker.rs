use std::mem;

use windows::{
    core::HSTRING,
    Win32::{
        Foundation::{ERROR_NOT_ENOUGH_MEMORY, HANDLE},
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            PSECURITY_DESCRIPTOR, SECURITY_DESCRIPTOR,
        },
        System::IO::OVERLAPPED,
    },
};

use crate::{
    heap::LocalPointer,
    named_pipe::{NamedPipe, NamedPipeResult},
};

// Pipe message format: u32 (native order) followed by message

enum PipeTalkerReadState {
    // no pending read, no pending operation
    Ready,
    // pending read, waiting for size; u32 = remaining bytes (of 4); vec: existing valid data (already read)
    PendingSized(u32, Vec<u8>),
    // no pending read, waiting for data; u32 = bytes needed
    WaitingForData(u32, Vec<u8>),
    // pending read, waiting for data; u32 = remaining bytes
    PendingData(u32, Vec<u8>),
    Error,
}

enum PipeTalkerWriteState {
    // no pending write; vec: queued bytes (not yet written)
    Queued(Vec<u8>),
    // pending write
    Pending(Vec<u8>),
    Error,
}

enum PipeTalkerConnectState {
    Ready,
    Pending,
    Error,
}

pub struct PipeTalker {
    read_state: PipeTalkerReadState,
    write_state: PipeTalkerWriteState,
    connect_state: PipeTalkerConnectState,
    pipe: NamedPipe,
    max_message_size: u32,
    max_write_queue_size: u32,
}

pub enum PipeAsyncResult<T> {
    Message(T),
    More,
    Blocked,
}

pub enum PipeTalkerResult {
    Read(Option<Box<[u8]>>),
    Written,
    Connected,
}

impl PipeTalker {
    fn create_sd(sddl: &str) -> windows::core::Result<LocalPointer<SECURITY_DESCRIPTOR>> {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let mut sdlen = 0u32;
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &mut sd,
                Some(&mut sdlen),
            )?;
        }
        unsafe { Ok(LocalPointer::from_raw_mut(sd.0.cast())) }
    }

    fn new(
        path: &str,
        max_message_size: u32,
        max_write_queue_size: u32,
        evented: bool,
        create: bool,
        create_sddl: Option<&str>,
    ) -> windows::core::Result<PipeTalker> {
        let pipe = if create {
            let sd = create_sddl.map(Self::create_sd).transpose()?;
            NamedPipe::create(path, true, evented, sd.as_deref().map(|x| (&x[0], false)))
        } else {
            assert!(create_sddl.is_none());
            NamedPipe::open(path, true, evented)
        }?;
        Ok(PipeTalker {
            read_state: PipeTalkerReadState::Ready,
            write_state: PipeTalkerWriteState::Queued(vec![]),
            connect_state: if create {
                PipeTalkerConnectState::Ready
            } else {
                PipeTalkerConnectState::Error
            },
            pipe,
            max_message_size,
            max_write_queue_size,
        })
    }

    pub fn create(
        path: &str,
        max_message_size: u32,
        max_write_queue_size: u32,
        evented: bool,
        sddl: Option<&str>,
    ) -> windows::core::Result<PipeTalker> {
        PipeTalker::new(
            path,
            max_message_size,
            max_write_queue_size,
            evented,
            true,
            sddl,
        )
    }

    pub fn open(
        path: &str,
        max_message_size: u32,
        max_write_queue_size: u32,
        evented: bool,
    ) -> windows::core::Result<PipeTalker> {
        PipeTalker::new(
            path,
            max_message_size,
            max_write_queue_size,
            evented,
            false,
            None,
        )
    }

    pub unsafe fn get_handle(&self) -> HANDLE {
        self.pipe.get_handle()
    }

    pub unsafe fn get_read_event(&self) -> Option<HANDLE> {
        self.pipe.get_read_event()
    }

    pub unsafe fn get_write_event(&self) -> Option<HANDLE> {
        self.pipe.get_write_event()
    }

    pub unsafe fn get_connect_event(&self) -> Option<HANDLE> {
        self.pipe.get_connect_event()
    }

    // if function returns bool, there's a completion being queued already
    pub fn begin_read(&mut self) -> windows::core::Result<bool> {
        let result: bool;
        self.read_state = match mem::replace(&mut self.read_state, PipeTalkerReadState::Error) {
            PipeTalkerReadState::Ready => {
                let count = size_of::<u32>() as u32;
                result = self.pipe.begin_read(count)?;
                windows::core::Result::Ok(PipeTalkerReadState::PendingSized(count, vec![]))
            }
            PipeTalkerReadState::WaitingForData(remain, data) => {
                result = self.pipe.begin_read(remain)?;
                Ok(PipeTalkerReadState::PendingData(remain, data))
            }
            _ => panic!("begin_read is inappropriate at this time"),
        }?;
        Ok(result)
    }

    fn on_read_message_size(
        &mut self,
        remain: u32,
        mut data: Vec<u8>,
        new_data: Box<[u8]>,
    ) -> windows::core::Result<PipeTalkerReadState> {
        assert!(new_data.len() <= remain as usize);
        data.extend_from_slice(&new_data);
        if new_data.len() < remain as usize {
            windows::core::Result::Ok(PipeTalkerReadState::PendingSized(
                remain - new_data.len() as u32,
                data,
            ))
        } else {
            match u32::from_ne_bytes(data.try_into().unwrap()) {
                // ping
                0 => Ok(PipeTalkerReadState::Ready),
                message_size => {
                    if message_size > self.max_message_size {
                        Err(ERROR_NOT_ENOUGH_MEMORY.into())
                    } else {
                        Ok(PipeTalkerReadState::WaitingForData(message_size, vec![]))
                    }
                }
            }
        }
    }

    fn on_read_data(
        &mut self,
        remain: u32,
        mut data: Vec<u8>,
        new_data: Box<[u8]>,
    ) -> windows::core::Result<(PipeTalkerReadState, Option<Box<[u8]>>)> {
        assert!(new_data.len() <= remain as usize);
        data.extend_from_slice(&new_data);
        if new_data.len() < remain as usize {
            windows::core::Result::Ok((
                PipeTalkerReadState::PendingData(remain - new_data.len() as u32, data),
                None,
            ))
        } else {
            Ok((PipeTalkerReadState::Ready, Some(data.into_boxed_slice())))
        }
    }

    fn complete_read(&mut self, new_data: Box<[u8]>) -> windows::core::Result<Option<Box<[u8]>>> {
        let mut result: Option<Box<[u8]>> = None;
        self.read_state = match mem::replace(&mut self.read_state, PipeTalkerReadState::Error) {
            PipeTalkerReadState::PendingSized(remain, data) => {
                self.on_read_message_size(remain, data, new_data)
            }
            PipeTalkerReadState::PendingData(remain, data) => {
                let (state, message) = self.on_read_data(remain, data, new_data)?;
                result = message;
                Ok(state)
            }
            _ => panic!("end_read is inappropriate at this time"),
        }?;
        Ok(result)
    }

    pub fn end_read(&mut self) -> windows::core::Result<Option<Box<[u8]>>> {
        let new_data = self.pipe.end_read_evented()?;
        self.complete_read(new_data)
    }

    fn do_queue_write(
        &mut self,
        message: Option<&[u8]>,
        mut remain: Vec<u8>,
        is_pending: bool,
    ) -> windows::core::Result<(bool, PipeTalkerWriteState)> {
        let mut result = false;
        if let Some(msg) = message {
            if msg.len() > self.max_message_size as usize {
                return Err(ERROR_NOT_ENOUGH_MEMORY.into());
            }
            remain.extend_from_slice((msg.len() as u32).to_ne_bytes().as_slice());
            remain.extend_from_slice(msg);
            if remain.len() > self.max_write_queue_size as usize {
                return Err(ERROR_NOT_ENOUGH_MEMORY.into());
            }
        }
        if !(is_pending || remain.is_empty()) {
            result = self.pipe.begin_write(remain.as_slice())?;
        }
        let state: PipeTalkerWriteState = if remain.is_empty() {
            PipeTalkerWriteState::Queued(remain)
        } else {
            PipeTalkerWriteState::Pending(remain)
        };
        Ok((result, state))
    }

    pub fn queue_write(&mut self, message: Option<&[u8]>) -> windows::core::Result<bool> {
        let (result, new_state) =
            match mem::replace(&mut self.write_state, PipeTalkerWriteState::Error) {
                PipeTalkerWriteState::Queued(remain) => self.do_queue_write(message, remain, false),
                PipeTalkerWriteState::Pending(remain) => self.do_queue_write(message, remain, true),
                _ => panic!("begin_write is inappropriate at this time"),
            }
            .inspect_err(|e| log::error!("do_queue_write {e}"))?;
        self.write_state = new_state;
        Ok(result)
    }

    fn complete_write(&mut self, written: u32) -> windows::core::Result<()> {
        self.write_state = match mem::replace(&mut self.write_state, PipeTalkerWriteState::Error) {
            PipeTalkerWriteState::Pending(mut remain) => {
                assert!(written as usize <= remain.len());
                remain.drain(0..written as usize);
                windows::core::Result::Ok(PipeTalkerWriteState::Queued(remain))
            }
            _ => panic!("end_write is inappropriate at this time"),
        }?;
        Ok(())
    }

    pub fn end_write(&mut self) -> windows::core::Result<()> {
        let count = self.pipe.end_write_evented()?;
        self.complete_write(count)
    }

    pub fn begin_connect(&mut self) -> windows::core::Result<bool> {
        let result: bool;
        self.connect_state =
            match mem::replace(&mut self.connect_state, PipeTalkerConnectState::Error) {
                PipeTalkerConnectState::Ready => {
                    result = self.pipe.begin_connect()?;
                    windows::core::Result::Ok(if result {
                        PipeTalkerConnectState::Ready
                    } else {
                        PipeTalkerConnectState::Pending
                    })
                }
                _ => panic!("begin_connect is inappropriate at this time"),
            }?;
        Ok(result)
    }

    fn complete_connect(&mut self) -> windows::core::Result<()> {
        self.connect_state =
            match mem::replace(&mut self.connect_state, PipeTalkerConnectState::Error) {
                PipeTalkerConnectState::Pending => {
                    windows::core::Result::Ok(PipeTalkerConnectState::Ready)
                }
                _ => panic!("end_connect is inappropriate at this time"),
            }?;
        Ok(())
    }

    pub fn end_connect(&mut self) -> windows::core::Result<()> {
        self.pipe.end_connect_evented()?;
        self.complete_connect()
    }

    pub unsafe fn complete_io(
        &mut self,
        overlapped: *const OVERLAPPED,
        result: windows::core::Result<u32>,
    ) -> windows::core::Result<PipeTalkerResult> {
        match self.pipe.complete_io(overlapped, result)? {
            NamedPipeResult::Read(new_data) => {
                Ok(PipeTalkerResult::Read(self.complete_read(new_data)?))
            }
            NamedPipeResult::Written(count) => {
                self.complete_write(count)?;
                Ok(PipeTalkerResult::Written)
            }
            NamedPipeResult::Connected => {
                self.complete_connect()?;
                Ok(PipeTalkerResult::Connected)
            }
        }
    }
}
