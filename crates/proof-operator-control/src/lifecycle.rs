use proof_operator_auth::OperatorAuthAuthority;

use crate::ControlShellError;

/// The two process signals that initiate the same fail-closed drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlSignal {
    Interrupt,
    Terminate,
}

/// Ordered shutdown seam implemented by the later real storage/runtime
/// assembly. Every cleanup method must be idempotent because the shell invokes
/// the complete tail even when an earlier stage fails.
pub trait ShutdownCoordinator {
    fn stop_listener_accepts(&mut self) -> Result<(), ControlShellError>;
    fn stop_new_permits(&mut self) -> Result<(), ControlShellError>;
    fn drain_mutations(&mut self) -> Result<(), ControlShellError>;
    fn release_pre_dispatch_reservations(&mut self) -> Result<(), ControlShellError>;
    fn checkpoint_durable_work(&mut self) -> Result<(), ControlShellError>;
    fn zeroize_runtime_custody(&mut self) -> Result<(), ControlShellError>;
    fn zeroize_cursor_and_signers(&mut self) -> Result<(), ControlShellError>;
    fn close_trusted_store(&mut self) -> Result<(), ControlShellError>;
    fn release_workspace_lock(&mut self) -> Result<(), ControlShellError>;
}

/// Executes the frozen drain and cleanup order. Once entered, it never returns
/// early: store close and workspace-lock release are attempted after every
/// intermediate failure, while the first failure remains the reported one.
pub fn shutdown_control_plane(
    authority: &OperatorAuthAuthority,
    coordinator: &mut dyn ShutdownCoordinator,
) -> Result<(), ControlShellError> {
    let mut failure = None;
    remember(&mut failure, coordinator.stop_listener_accepts());
    remember(&mut failure, coordinator.stop_new_permits());
    remember(&mut failure, coordinator.drain_mutations());
    remember(
        &mut failure,
        coordinator.release_pre_dispatch_reservations(),
    );
    remember(&mut failure, coordinator.checkpoint_durable_work());
    remember(
        &mut failure,
        authority
            .invalidate_for_shutdown()
            .map_err(|_| ControlShellError::ControlUnavailable),
    );
    remember(&mut failure, coordinator.zeroize_runtime_custody());
    remember(&mut failure, coordinator.zeroize_cursor_and_signers());
    remember(&mut failure, coordinator.close_trusted_store());
    remember(&mut failure, coordinator.release_workspace_lock());
    failure.map_or(Ok(()), Err)
}

/// SIGINT and SIGTERM deliberately share the exact shutdown implementation.
pub fn shutdown_for_signal(
    signal: ControlSignal,
    authority: &OperatorAuthAuthority,
    coordinator: &mut dyn ShutdownCoordinator,
) -> Result<(), ControlShellError> {
    match signal {
        ControlSignal::Interrupt | ControlSignal::Terminate => {
            shutdown_control_plane(authority, coordinator)
        }
    }
}

/// Waits for the first supported Unix termination signal. Failure to install a
/// signal handler is fatal instead of leaving the process serving without a
/// reliable cleanup path.
#[cfg(target_os = "linux")]
pub async fn wait_for_control_signal() -> Result<ControlSignal, ControlShellError> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut interrupt =
        signal(SignalKind::interrupt()).map_err(|_| ControlShellError::ControlUnavailable)?;
    let mut terminate =
        signal(SignalKind::terminate()).map_err(|_| ControlShellError::ControlUnavailable)?;
    tokio::select! {
        value = interrupt.recv() => value
            .map(|_| ControlSignal::Interrupt)
            .ok_or(ControlShellError::ControlUnavailable),
        value = terminate.recv() => value
            .map(|_| ControlSignal::Terminate)
            .ok_or(ControlShellError::ControlUnavailable),
    }
}

#[cfg(not(target_os = "linux"))]
pub async fn wait_for_control_signal() -> Result<ControlSignal, ControlShellError> {
    Err(ControlShellError::UnsupportedPlatform)
}

fn remember(failure: &mut Option<ControlShellError>, result: Result<(), ControlShellError>) {
    if let Err(error) = result {
        if failure.is_none() {
            *failure = Some(error);
        }
    }
}
