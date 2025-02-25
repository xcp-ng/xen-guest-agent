extern crate windows;
extern crate windows_service;

use std::sync::mpsc;
use std::time::Duration;

use clap::Parser;
use windows::Win32::Foundation::{ERROR_INVALID_PARAMETER, ERROR_SUCCESS};

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

use crate::{run_async, GuestAgentConfig};

const SERVICE_NAME: &str = "xenguestagent-rs";

fn service_main() -> anyhow::Result<()> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                log::info!("Sending service stop message");
                stop_tx.send(()).unwrap();
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    {
        let status = ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP,
            exit_code: ServiceExitCode::Win32(ERROR_SUCCESS.0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        };
        status_handle.set_service_status(status)?;
    }
    log::info!("Service starting");

    let builder = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let service_result = builder.block_on(async {
        let config = GuestAgentConfig::parse();
        let mut set = run_async(&config).await?;
        log::info!("Service started");
        stop_rx.recv()?;
        log::info!("Service stopping");
        set.shutdown().await;
        anyhow::Result::<()>::Ok(())
    });
    match service_result {
        Ok(_) => log::info!("Service returned successfully"),
        Err(ref e) => log::error!("Service returned error {e}"),
    }

    {
        let status = ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: if service_result.is_ok() {
                ServiceExitCode::Win32(ERROR_SUCCESS.0)
            } else {
                ServiceExitCode::Win32(ERROR_INVALID_PARAMETER.0)
            },
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        };
        status_handle.set_service_status(status)?;
    }

    Ok(())
}

extern "system" fn ffi_service_main(
    _num_service_arguments: u32,
    _service_arguments: *mut *mut u16,
) {
    match service_main() {
        Ok(_) => (),
        Err(ref e) => log::error!("Service start encountered an error {e}"),
    }
}

pub(crate) fn dispatch_main() -> anyhow::Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}
