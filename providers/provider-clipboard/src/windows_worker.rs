use std::collections::HashMap;

use guest_metrics::ClipboardData;
use windows::{
    core::Owned,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Pipes::GetNamedPipeClientSessionId,
            RemoteDesktop::WTSGetActiveConsoleSessionId,
            Threading::INFINITE,
            IO::{CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED},
        },
    },
};
use xen_win_utils::{
    iocp::EventCompletion,
    overlapped::clear_io_completion_port,
    pipe_talker::{PipeTalker, PipeTalkerResult},
};

const INVALID_SESSION_ID: u32 = 0xffffffff;

const FREE_CLIENT_KEY: usize = usize::MAX;
const SET_CLIPBOARD_SIGNALED: usize = usize::MAX - 1;
const STOP_SIGNALED: usize = usize::MAX - 2;

// limit the creation of pipe instances with a magic prefix
const CLIPBOARD_PIPE_SERVER_PATH: &str =
    r"\\.\pipe\ProtectedPrefix\Administrators\XenWinClipboardService";
// grant FILE_GENERIC_READ | FILE_WRITE_DATA to NT AUTHORITY\INTERACTIVE
// technically FILE_GENERIC_WRITE is of no concern thanks to ProtectedPrefix\Administrators,
// but a custom access mask doesn't cost anything
// no need to specify an owner, it errors out otherwise
const CLIPBOARD_PIPE_SERVER_SDDL: &str = r"D:(A;;0x12008b;;;IU)(A;;FA;;;SY)(A;;FA;;;BA)";
// arbitrary size limits
const MAX_MESSAGE_SIZE: u32 = 65535;
const MAX_WRITE_QUEUE_SIZE: u32 = 262143;

struct CompletionPacket {
    overlapped: *mut OVERLAPPED,
    key: usize,
    result: windows::core::Result<u32>,
}

pub(crate) struct WindowsClipboardWorker {
    stop_completion: EventCompletion,
    sender: flume::Sender<ClipboardData>,
    receiver: flume::Receiver<ClipboardData>,
    recv_completion: EventCompletion,
    free_client: Option<PipeTalker>,
    clients: HashMap<u32, PipeTalker>,
    completion_port: Owned<HANDLE>,
}

unsafe impl Send for WindowsClipboardWorker {}

impl WindowsClipboardWorker {
    pub(crate) fn new(
        stop_event: HANDLE,
        sender: flume::Sender<ClipboardData>,
        receiver: flume::Receiver<ClipboardData>,
        recv_event: HANDLE,
    ) -> windows::core::Result<Self> {
        assert!(
            FREE_CLIENT_KEY != INVALID_SESSION_ID as usize,
            "32-bit systems are not supported"
        );
        let completion_port =
            unsafe { Owned::new(CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 0)?) };

        let stop_completion = EventCompletion::new(stop_event)?;
        let recv_completion = EventCompletion::new(recv_event)?;

        Ok(Self {
            stop_completion,
            sender,
            receiver,
            recv_completion,
            free_client: None,
            clients: HashMap::<u32, PipeTalker>::new(),
            completion_port,
        })
    }

    unsafe fn get_client_sid(pipe: HANDLE) -> windows::core::Result<u32> {
        let mut sid = 0u32;
        unsafe { GetNamedPipeClientSessionId(pipe, &mut sid)? };
        Ok(sid)
    }

    fn on_client_connected(&mut self, new_client: PipeTalker) -> windows::core::Result<()> {
        unsafe {
            clear_io_completion_port(new_client.get_handle())?;
        }
        // TODO: review get_client_sid failure?
        let sid = unsafe { Self::get_client_sid(new_client.get_handle())? };
        if let Some(_) = self.clients.insert(sid, new_client) {}
        let new_client = self.clients.get_mut(&sid).unwrap();
        unsafe {
            CreateIoCompletionPort(
                new_client.get_handle(),
                Some(*self.completion_port),
                sid as usize,
                0,
            )?;
        }
        new_client.begin_read()?;
        Ok(())
    }

    fn get_active_console_session_id() -> windows::core::Result<u32> {
        let active = unsafe { WTSGetActiveConsoleSessionId() };
        match active {
            INVALID_SESSION_ID => Err(ERROR_FILE_NOT_FOUND.into()),
            _ => Ok(active),
        }
    }

    fn send_host_msg_to_client(
        &mut self,
        msg: ClipboardData,
    ) -> (u32, windows::core::Result<bool>) {
        let active = Self::get_active_console_session_id().unwrap_or(INVALID_SESSION_ID);
        (
            active,
            match self.clients.get_mut(&active) {
                Some(client) => client.queue_write(Some(&msg)),
                None => Err(ERROR_FILE_NOT_FOUND.into()),
            },
        )
    }

    fn on_client_read(sender: &flume::Sender<ClipboardData>, key: usize, msg: ClipboardData) {
        let key = key;
        match Self::get_active_console_session_id() {
            Ok(active) if active as usize == key => sender.send(msg).unwrap(),
            _ => (),
        }
    }

    fn complete_client(
        &mut self,
        completion_packet: CompletionPacket,
    ) -> windows::core::Result<()> {
        let key = completion_packet.key;
        let client = self.clients.get_mut(&(key as u32)).unwrap();
        match unsafe { client.complete_io(completion_packet.overlapped, completion_packet.result)? }
        {
            // pump the pipe again
            PipeTalkerResult::Read(read) => {
                if let Some(message) = read {
                    Self::on_client_read(&self.sender, key, message);
                }
                client.begin_read()?;
            }
            PipeTalkerResult::Written => {
                client.queue_write(None)?;
            }
            _ => panic!("unexpected completion event"),
        }
        Ok(())
    }

    fn check_free_client(&mut self) -> anyhow::Result<()> {
        self.free_client = match self.free_client.take() {
            Some(c) => Some(c),
            None => {
                let mut new_client = PipeTalker::create(
                    CLIPBOARD_PIPE_SERVER_PATH,
                    MAX_MESSAGE_SIZE,
                    MAX_WRITE_QUEUE_SIZE,
                    false,
                    Some(CLIPBOARD_PIPE_SERVER_SDDL),
                )?;
                unsafe {
                    CreateIoCompletionPort(
                        new_client.get_handle(),
                        Some(*self.completion_port),
                        FREE_CLIENT_KEY,
                        0,
                    )?;
                }
                if new_client.begin_connect()? {
                    self.on_client_connected(new_client)?;
                    None
                } else {
                    Some(new_client)
                }
            }
        };
        Ok(())
    }

    fn complete_free_client(
        &mut self,
        completion_packet: CompletionPacket,
    ) -> windows::core::Result<()> {
        let mut new_client = self.free_client.take().unwrap();
        match unsafe {
            new_client.complete_io(completion_packet.overlapped, completion_packet.result)?
        } {
            PipeTalkerResult::Connected => {
                self.on_client_connected(new_client)?;
            }
            _ => panic!("unexpected free client completion at this state"),
        };
        Ok(())
    }

    fn wait_completion_packet(&mut self) -> windows::core::Result<CompletionPacket> {
        let mut bytes = 0u32;
        let mut key = 0;
        let mut overlapped: *mut OVERLAPPED = std::ptr::null_mut();
        match unsafe {
            GetQueuedCompletionStatus(
                *self.completion_port,
                &mut bytes,
                &mut key,
                &mut overlapped,
                INFINITE,
            )
        } {
            Ok(_) => Ok(CompletionPacket {
                overlapped,
                key,
                result: Ok(bytes),
            }),
            Err(e) => {
                if overlapped.is_null() {
                    Err(e)
                } else {
                    Ok(CompletionPacket {
                        overlapped,
                        key,
                        result: Err(e),
                    })
                }
            }
        }
    }

    pub(crate) fn run(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.stop_completion
                .rearm(*self.completion_port, STOP_SIGNALED)?;
            self.recv_completion
                .rearm(*self.completion_port, SET_CLIPBOARD_SIGNALED)?;
        }

        loop {
            // begin of day: set up new free PipeTalker for clients to take if needed
            self.check_free_client()?;

            // wait for something interesting to happen
            let completion_packet = self.wait_completion_packet()?;

            let key = completion_packet.key;
            // what kind of event is it?
            let client_result: Result<(), (windows::core::Error, usize)> = match key {
                FREE_CLIENT_KEY => self
                    .complete_free_client(completion_packet)
                    .map_err(|e| (e, key)),
                SET_CLIPBOARD_SIGNALED => {
                    unsafe {
                        self.recv_completion
                            .rearm(*self.completion_port, SET_CLIPBOARD_SIGNALED)?;
                    }
                    // obviously cannot set clipboard more than once per event; consume everything
                    if let Some(host_msg) = self.receiver.drain().last() {
                        match self.send_host_msg_to_client(host_msg) {
                            (_, Ok(_)) => Ok(()),
                            (active, Err(e)) => Err((e, active as usize)),
                        }
                    } else {
                        Ok(())
                    }
                }
                STOP_SIGNALED => {
                    log::debug!("Clipboard stop signaled");
                    break;
                }
                _ => self
                    .complete_client(completion_packet)
                    .map_err(|e| (e, key)),
            };

            // remove the ones that failed
            if let Err((_e, failed_key)) = client_result {
                match failed_key {
                    FREE_CLIENT_KEY => {
                        self.free_client.take();
                    }
                    SET_CLIPBOARD_SIGNALED => panic!("signal path cannot be a failed_key"),
                    _ => {
                        self.clients.remove(&(failed_key as u32));
                    }
                }
            }
        }

        log::debug!("Leaving clipboard main loop");
        Ok(())
    }
}
