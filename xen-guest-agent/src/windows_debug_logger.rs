use log::Log;
use windows::{core::HSTRING, Win32::System::Diagnostics::Debug::OutputDebugStringW};

pub(crate) struct WindowsDebugLogger {}

impl Log for WindowsDebugLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let message = format!(
            "[xen-guest-agent] {}: {}\r\n",
            record.level().as_str(),
            record.args()
        );
        unsafe {
            OutputDebugStringW(&HSTRING::from(message));
        }
    }

    fn flush(&self) {}
}
