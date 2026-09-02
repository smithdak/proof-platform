use thiserror::Error;

/// Closed, non-secret failures produced by the W4 control shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ControlShellError {
    #[error("operator control is unavailable")]
    ControlUnavailable,
    #[error("the listener is unavailable")]
    ListenerUnavailable,
    #[error("the static application is invalid")]
    StaticBundleInvalid,
    #[error("the terminal ceremony failed")]
    TerminalUnavailable,
    #[error("the workspace is unsafe")]
    UnsafeWorkspace,
    #[error("operator control is unsupported on this host")]
    UnsupportedPlatform,
}
