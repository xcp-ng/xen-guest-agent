use log::Log;
use windows::{core::HSTRING, Win32::System::Diagnostics::Debug::OutputDebugStringW};

pub struct WindowsDebugLogger {
    pub prefix: String,
}

impl Log for WindowsDebugLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let message = format!(
            "{} {}: {}\r\n",
            self.prefix,
            record.level().as_str(),
            record.args()
        );
        unsafe {
            OutputDebugStringW(&HSTRING::from(message));
        }
    }

    fn flush(&self) {}
}
