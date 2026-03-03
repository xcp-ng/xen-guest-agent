#![windows_subsystem = "windows"]

mod clipboard;

use std::process::ExitCode;

use clipboard::Clipboard;
use xen_win_utils::{
    named_mutex::NamedMutexGuard,
    overlapped::{windowed_wait, WindowedWaitResult},
    pipe_talker::PipeTalker,
};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{WPARAM, *},
        System::{
            DataExchange::*,
            LibraryLoader::GetModuleHandleW,
            Ole::CF_UNICODETEXT,
            Recovery::{RegisterApplicationRestart, RESTART_NO_REBOOT},
            Threading::INFINITE,
        },
        UI::WindowsAndMessaging::*,
    },
};

use xen_win_utils::windows_debug_logger::WindowsDebugLogger;

const CLASS_NAME: PCWSTR = w!("XenWinClipboardAgent");
const CLIPBOARD_PIPE_SERVER_PATH: &str =
    r"\\.\pipe\ProtectedPrefix\Administrators\XenWinClipboardService";
const MAX_MESSAGE_SIZE: u32 = 65535;
const MAX_WRITE_QUEUE_SIZE: u32 = 262143;

struct App {
    client: Option<PipeTalker>,
    hwnd: HWND,
}

impl App {
    fn new() -> windows::core::Result<Box<Self>> {
        let hwnd = unsafe {
            let wcex = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(Self::wndproc),
                hInstance: GetModuleHandleW(None)?.into(),
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };

            let atom = RegisterClassExW(&wcex);
            if atom == 0 {
                return Err(windows::core::Error::from_thread().into());
            }

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS_NAME,
                None,
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )?;

            hwnd
        };

        let mut app = Box::new(Self { hwnd, client: None });
        let _ = app
            .reconnect()
            .inspect_err(|e| log::error!("Cannot open initial connection: {e}"));
        unsafe { SetWindowLongPtrW(app.hwnd, GWLP_USERDATA, &mut *app as *mut Self as isize) };

        Ok(app)
    }

    fn reconnect(&mut self) -> windows::core::Result<()> {
        let hwnd = self.hwnd;
        let client = PipeTalker::open(
            CLIPBOARD_PIPE_SERVER_PATH,
            MAX_MESSAGE_SIZE,
            MAX_WRITE_QUEUE_SIZE,
            true,
        )
        .inspect_err(|e| log::debug!("Cannot open pipe: {e}"))?;
        self.client.replace(client);

        let _ = self
            .with_pipe(|client| {
                while client.begin_read()? {
                    if let Some(msg) = client.end_read()? {
                        Self::on_pipe_msg(hwnd, msg)?;
                    }
                }
                Ok(())
            })
            .inspect_err(|e| log::error!("Pipe initial pump failed: {e}"));

        Ok(())
    }

    fn with_pipe(
        &mut self,
        f: impl FnOnce(&mut PipeTalker) -> windows::core::Result<()>,
    ) -> windows::core::Result<bool> {
        if let Some(client) = self.client.as_mut() {
            match f(client) {
                Ok(_) => Ok(true),
                Err(e) => {
                    log::error!("Pipe error, closing: {e}");
                    drop(self.client.take());
                    Err(e)
                }
            }
        } else {
            Ok(false)
        }
    }

    fn on_clipboard_update(
        &mut self,
        hwnd: HWND,
        _msg: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> windows::core::Result<LRESULT> {
        if let Err(_) = unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32) } {
            return Ok(LRESULT(0));
        }

        let cb = Clipboard::new(hwnd)?;
        let cb_text = cb.get_wide_z()?;

        // TODO: convert to ipc byte format? rmp?
        // replicate String::from_utf16_lossy and break at null at the same time
        let str: String = char::decode_utf16(cb_text.iter().copied().take_while(|c| *c != 0u16))
            .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect();
        let _ = self
            .with_pipe(|client| {
                if client.queue_write(Some(str.as_bytes()))? {
                    while client.queue_write(None)? {}
                }
                Ok(())
            })
            .inspect_err(|e| log::error!("Clipboard update failed: {e}"));

        Ok(LRESULT(0))
    }

    extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                WM_CREATE => {
                    if let Err(e) = AddClipboardFormatListener(hwnd) {
                        panic!("AddClipboardFormatListener error {e}");
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
                WM_CLIPBOARDUPDATE => {
                    let this = (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Self)
                        .as_mut()
                        .unwrap();

                    if let Err(e) = this.on_clipboard_update(hwnd, msg, wparam, lparam) {
                        log::error!("WM_CLIPBOARDUPDATE error {e}");
                    }
                    LRESULT(0)
                }
                WM_CLOSE => {
                    let _ = RemoveClipboardFormatListener(hwnd);
                    let _ = DestroyWindow(hwnd);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }

    // None to continue, or Some(ExitCode) to exit
    fn on_window_msg(&self, msg: &mut MSG) -> Option<ExitCode> {
        while unsafe { PeekMessageW(msg, None, 0, 0, PM_REMOVE) } == TRUE {
            if msg.message == WM_QUIT {
                return Some(ExitCode::from(msg.wParam.0.try_into().unwrap_or(1)));
            }
            unsafe {
                let _ = DispatchMessageW(msg);
            }
        }
        None
    }

    fn on_pipe_msg(hwnd: HWND, msg: Box<[u8]>) -> windows::core::Result<()> {
        // TODO: convert to ipc byte format? rmp?
        let mut cb_z: Vec<u16> = String::from_utf8_lossy(&msg).encode_utf16().collect();
        cb_z.push(0);

        let cb = Clipboard::new(hwnd)?;
        cb.set_wide_z(&cb_z)?;
        Ok(())
    }

    fn run(&mut self) -> windows::core::Result<ExitCode> {
        const RECONNECT_DELAY_MSEC: u32 = 10000;

        loop {
            let mut msg = MSG::default();
            let mut handles: Option<[HANDLE; 2]> = None;
            let mut timeout = RECONNECT_DELAY_MSEC;

            let _ = self.with_pipe(|client| {
                handles = Some(unsafe {
                    [
                        client.get_read_event().unwrap(),
                        client.get_write_event().unwrap(),
                    ]
                });
                timeout = INFINITE;

                Ok(())
            });

            match windowed_wait(
                handles.as_ref().map(|h| h.as_slice()),
                timeout,
                QS_ALLINPUT,
                false,
                true,
                false,
            )? {
                WindowedWaitResult::Input => {
                    if let Some(value) = self.on_window_msg(&mut msg) {
                        return Ok(value);
                    }
                }
                // pump the pipe again
                WindowedWaitResult::Handle(0) => {
                    let hwnd = self.hwnd;
                    let _ = self
                        .with_pipe(|client| {
                            if let Some(msg) = client.end_read()? {
                                Self::on_pipe_msg(hwnd, msg)?;
                            }
                            while client.begin_read()? {
                                if let Some(msg) = client.end_read()? {
                                    Self::on_pipe_msg(hwnd, msg)?;
                                }
                            }
                            Ok(())
                        })
                        .inspect_err(|e| log::error!("Pipe read failed: {e}"));
                }
                WindowedWaitResult::Handle(1) => {
                    let _ = self
                        .with_pipe(|client| {
                            client.end_write()?;
                            while client.queue_write(None)? {}
                            Ok(())
                        })
                        .inspect_err(|e| log::error!("Pipe write failed: {e}"));
                }
                WindowedWaitResult::Timeout => {
                    if self.client.is_none() {
                        // Reconnection may fail
                        let _ = self.reconnect();
                    }
                }
                _ => {
                    log::error!("Unexpected windowed wait state");
                }
            }
        }
    }
}

fn main() -> anyhow::Result<ExitCode> {
    log::set_boxed_logger(Box::new(WindowsDebugLogger {
        prefix: "[xen-win-clipboard]".to_string(),
    }))?;
    #[cfg(debug_assertions)]
    log::set_max_level(log::LevelFilter::Trace);
    #[cfg(not(debug_assertions))]
    log::set_max_level(log::LevelFilter::Info);

    let single = NamedMutexGuard::new(Some("Local\\XenWinClipboardAgent"), true)?;
    if let None = single {
        log::info!("Another instance is already running");
        return Ok(ExitCode::from(ERROR_ALREADY_EXISTS.0 as u8));
    }

    unsafe { RegisterApplicationRestart(PCWSTR::null(), RESTART_NO_REBOOT)? };

    let mut app = App::new()?;
    Ok(app.run()?)
}
