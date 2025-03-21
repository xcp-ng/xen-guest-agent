use std::thread::JoinHandle;

use futures::{select, FutureExt, StreamExt};
use guest_metrics::{plugin::GuestAgentPlugin, GuestMetric};
use windows::{
    core::Free,
    Win32::{
        Foundation::HANDLE,
        System::Threading::{CreateEventW, SetEvent},
    },
};
use xenstore_rs::{AsyncWatch, AsyncXs};
use xenstore_win::smol::XsSmolWindows;

use crate::windows_worker::WindowsClipboardWorker;

// used for sending event handles across await boundary
#[derive(Clone)]
struct SendHandle(HANDLE);
unsafe impl Send for SendHandle {}

struct OwnedSendHandle(HANDLE);
unsafe impl Send for OwnedSendHandle {}
impl Drop for OwnedSendHandle {
    fn drop(&mut self) {
        unsafe { self.0.free() }
    }
}

struct WindowsClipboardPluginState {
    worker_thread: JoinHandle<anyhow::Result<()>>,
    stop_event: OwnedSendHandle,
}

pub struct WindowsClipboardPlugin {
    state: Option<WindowsClipboardPluginState>,
}

impl WindowsClipboardPlugin {
    pub fn new() -> anyhow::Result<WindowsClipboardPlugin> {
        Ok(WindowsClipboardPlugin { state: None })
    }

    async fn rm_set_clipboard(xs: &impl AsyncXs) {
        let _ = xs
            .rm("data/set_clipboard")
            .await
            .inspect_err(|e| log::error!("cannot clean up set_clipboard: {e}"));
    }

    async fn do_set_clipboard(
        xs: &impl AsyncXs,
        lines: &mut Vec<String>,
        my_sender: &flume::Sender<Box<[u8]>>,
        send_event: SendHandle,
    ) -> anyhow::Result<()> {
        let xs_data = xs
            .read("data/set_clipboard")
            .await
            .inspect_err(|e| log::error!("cannot receive set_clipboard: {e}"))
            .unwrap_or(String::new().into());
        Self::rm_set_clipboard(xs).await;

        if xs_data.is_empty() {
            let mut multiline = String::new();
            for line in &mut *lines {
                multiline.push_str(&line);
                multiline.push_str("\r\n");
            }
            let _ = my_sender
                .send_async(multiline.into_boxed_str().into())
                .await
                .inspect_err(|e| log::error!("cannot send clipboard to client: {e}"));
            lines.clear();
            unsafe { SetEvent(send_event.0)? };
        } else {
            lines.push(xs_data.into_string());
        }

        Ok(())
    }

    pub async fn do_run(&mut self, provider: flume::Sender<GuestMetric>) -> anyhow::Result<()> {
        log::info!("Starting clipboard worker");
        // for sending SetClipboard to guest
        let (my_sender, their_receiver) = flume::bounded(1);
        // for receiving GetClipboard from guest
        let (their_sender, my_receiver) = flume::bounded(1);

        let xs_smol = XsSmolWindows::new()
            .await
            .expect("Unable to start async xenstore");
        Self::rm_set_clipboard(&xs_smol).await;

        let mut clipboard_watch = xs_smol
            .watch("data/set_clipboard")
            .await
            .expect("Unable to watch clipboard node");

        let stop_event = unsafe { OwnedSendHandle(CreateEventW(None, false, false, None)?) };
        let send_event = unsafe { OwnedSendHandle(CreateEventW(None, false, false, None)?) };
        let mut worker =
            WindowsClipboardWorker::new(stop_event.0, their_sender, their_receiver, send_event.0)?;

        assert!(self.state.is_none());
        self.state.replace(WindowsClipboardPluginState {
            worker_thread: std::thread::spawn(move || -> anyhow::Result<()> {
                worker
                    .run()
                    .inspect_err(|e| log::error!("worker error {e}"))
            }),
            stop_event,
        });

        let mut lines = Vec::<String>::new();
        loop {
            let mut xs_fut = clipboard_watch.next().fuse();
            let mut guest_fut = my_receiver.recv_async().fuse();

            select! {
                _ = xs_fut => {
                    Self::do_set_clipboard(&xs_smol, &mut lines, &my_sender, SendHandle(send_event.0)).await?;
                }
                guest_data = guest_fut => {
                    if let Ok(guest_clipboard) = guest_data {
                        provider.send_async(GuestMetric::GetClipboard(guest_clipboard)).await?;
                    }
                }
                complete => break,
            }
        }
        Ok(())
    }
}

impl Drop for WindowsClipboardPlugin {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            log::debug!("Stopping clipboard worker thread");
            unsafe {
                let _ = SetEvent(state.stop_event.0)
                    .inspect_err(|e| log::error!("cannot send clipboard worker stop event: {e}"));
            };
            let _ = state
                .worker_thread
                .join()
                .inspect_err(|e| log::info!("clipboard worker error: {e:?}"));
            log::debug!("Clipboard worker thread stopped");
        }
    }
}

impl GuestAgentPlugin for WindowsClipboardPlugin {
    async fn run(mut self, channel: flume::Sender<GuestMetric>) {
        self.do_run(channel).await.expect("clipboard plugin failed");
    }
}
