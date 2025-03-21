use windows::{
    Wdk::System::SystemServices::RtlGetVersion, Win32::System::SystemInformation::OSVERSIONINFOW,
};

pub mod heap;
pub mod iocp;
pub mod named_mutex;
pub mod named_pipe;
pub mod overlapped;
pub mod pipe_talker;
pub mod windows_debug_logger;

pub fn get_version() -> windows::core::Result<String> {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    unsafe {
        let ntstatus = RtlGetVersion(&mut version);
        ntstatus.ok()?;
    }
    Ok(format!(
        "{0}.{1}.{2}",
        version.dwMajorVersion, version.dwMinorVersion, version.dwBuildNumber
    ))
}
