use std::{
    fs::{File, OpenOptions},
    io::{ErrorKind, Read, Write},
    thread,
    time::{Duration, Instant},
};

use proof_kernel::Capability;
use proof_operator_auth::{
    challenge_code, OperatorAuthAuthority, OperatorAuthError, SessionAttestation, SessionChallenge,
};
use subtle::ConstantTimeEq;
use thiserror::Error;
use zeroize::Zeroizing;

/// Descriptor-backed challenge signer. Implementations own key cleanup and tuple revalidation.
pub trait ChallengeSigner: Send + Sync {
    fn sign_challenge(
        &self,
        challenge: &SessionChallenge,
    ) -> Result<SessionAttestation, TerminalCeremonyError>;
}

/// Narrow controlling-terminal interface used by the one-attempt ceremony.
pub trait ControllingTerminal {
    fn write_nonsecret(&mut self, text: &str) -> Result<(), TerminalCeremonyError>;
    fn set_echo(&mut self, enabled: bool) -> Result<(), TerminalCeremonyError>;
    fn read_confirmation(
        &mut self,
        maximum_bytes: usize,
        timeout: Duration,
    ) -> Result<String, TerminalCeremonyError>;
}

/// The launcher-owned controlling terminal, opened before listener bind.
pub struct OsControllingTerminal {
    file: File,
    original: Option<(rustix::termios::Termios, rustix::fs::OFlags)>,
}

impl OsControllingTerminal {
    /// Opens `/dev/tty` directly. Pipes and inherited standard streams are not used.
    pub fn open() -> Result<Self, TerminalCeremonyError> {
        #[cfg(not(target_os = "linux"))]
        return Err(TerminalCeremonyError::TerminalUnavailable);

        #[cfg(target_os = "linux")]
        {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/tty")
                .map_err(|_| TerminalCeremonyError::TerminalUnavailable)?;
            rustix::termios::tcgetattr(&file)
                .map_err(|_| TerminalCeremonyError::TerminalUnavailable)?;
            Ok(Self {
                file,
                original: None,
            })
        }
    }

    /// Verifies before listener bind that echo can be disabled and restored.
    pub fn verify_echo_restoration(&mut self) -> Result<(), TerminalCeremonyError> {
        self.set_echo(false)?;
        if self.set_echo(true).is_err() {
            return Err(TerminalCeremonyError::RestorationFailed);
        }
        Ok(())
    }
}

impl ControllingTerminal for OsControllingTerminal {
    fn write_nonsecret(&mut self, text: &str) -> Result<(), TerminalCeremonyError> {
        self.file
            .write_all(text.as_bytes())
            .and_then(|()| self.file.flush())
            .map_err(|_| TerminalCeremonyError::TerminalUnavailable)
    }

    fn set_echo(&mut self, enabled: bool) -> Result<(), TerminalCeremonyError> {
        use rustix::termios::{tcgetattr, tcsetattr, LocalModes, OptionalActions};

        if enabled {
            let (termios, flags) = self
                .original
                .as_ref()
                .ok_or(TerminalCeremonyError::TerminalUnavailable)?;
            tcsetattr(&self.file, OptionalActions::Now, termios)
                .map_err(|_| TerminalCeremonyError::RestorationFailed)?;
            rustix::fs::fcntl_setfl(&self.file, *flags)
                .map_err(|_| TerminalCeremonyError::RestorationFailed)?;
            self.original.take();
            Ok(())
        } else {
            if self.original.is_some() {
                return Err(TerminalCeremonyError::TerminalUnavailable);
            }
            let original =
                tcgetattr(&self.file).map_err(|_| TerminalCeremonyError::TerminalUnavailable)?;
            let flags = rustix::fs::fcntl_getfl(&self.file)
                .map_err(|_| TerminalCeremonyError::TerminalUnavailable)?;
            let mut guarded = original.clone();
            guarded
                .local_modes
                .remove(LocalModes::ECHO | LocalModes::ECHONL);
            tcsetattr(&self.file, OptionalActions::Now, &guarded)
                .map_err(|_| TerminalCeremonyError::TerminalUnavailable)?;
            self.original = Some((original, flags));
            if rustix::fs::fcntl_setfl(&self.file, flags | rustix::fs::OFlags::NONBLOCK).is_err() {
                return match self.set_echo(true) {
                    Ok(()) => Err(TerminalCeremonyError::TerminalUnavailable),
                    Err(_) => Err(TerminalCeremonyError::RestorationFailed),
                };
            }
            Ok(())
        }
    }

    fn read_confirmation(
        &mut self,
        maximum_bytes: usize,
        timeout: Duration,
    ) -> Result<String, TerminalCeremonyError> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(maximum_bytes.min(64)));
        let mut next = [0_u8; 1];
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(TerminalCeremonyError::TerminalUnavailable)?;
        loop {
            if Instant::now() >= deadline {
                return Err(TerminalCeremonyError::TimedOut);
            }
            if bytes.len() >= maximum_bytes {
                return Err(TerminalCeremonyError::AuthorizationMismatch);
            }
            match self.file.read(&mut next) {
                Ok(0) => return Err(TerminalCeremonyError::TerminalUnavailable),
                Ok(_) => {
                    bytes.push(next[0]);
                    if next[0] == b'\n' {
                        break;
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(TerminalCeremonyError::TimedOut);
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return Err(TerminalCeremonyError::TerminalUnavailable),
            }
        }
        std::str::from_utf8(bytes.as_slice())
            .map(str::to_owned)
            .map_err(|_| TerminalCeremonyError::AuthorizationMismatch)
    }
}

impl Drop for OsControllingTerminal {
    fn drop(&mut self) {
        if let Some((termios, flags)) = self.original.take() {
            let _ = rustix::termios::tcsetattr(
                &self.file,
                rustix::termios::OptionalActions::Now,
                &termios,
            );
            let _ = rustix::fs::fcntl_setfl(&self.file, flags);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TerminalCeremonyError {
    #[error("authorization did not match")]
    AuthorizationMismatch,
    #[error("challenge signing failed")]
    SigningFailed,
    #[error("the controlling terminal is unavailable")]
    TerminalUnavailable,
    #[error("the terminal confirmation timed out")]
    TimedOut,
    #[error("terminal echo restoration failed")]
    RestorationFailed,
}

/// Performs one non-echoed cross-channel confirmation and submits one attestation.
pub fn complete_challenge_ceremony(
    authority: &OperatorAuthAuthority,
    challenge: &SessionChallenge,
    terminal: &mut dyn ControllingTerminal,
    signer: &dyn ChallengeSigner,
) -> Result<(), TerminalCeremonyError> {
    complete_challenge_ceremony_with_timeout(
        authority,
        challenge,
        terminal,
        signer,
        Duration::from_secs(120),
    )
}

/// Performs the ceremony with an explicit bounded timeout. Production uses
/// the fixed challenge lifetime; deterministic tests may supply a shorter
/// timeout without changing the signed challenge.
pub(crate) fn complete_challenge_ceremony_with_timeout(
    authority: &OperatorAuthAuthority,
    challenge: &SessionChallenge,
    terminal: &mut dyn ControllingTerminal,
    signer: &dyn ChallengeSigner,
    timeout: Duration,
) -> Result<(), TerminalCeremonyError> {
    let mut attempt = ChallengeAttempt::new(authority, challenge.challenge_id);
    let capabilities = challenge
        .granted_capabilities
        .iter()
        .map(capability_label)
        .collect::<Vec<_>>()
        .join(", ");
    let prompt = format!(
        "Workspace: {}\nOrigin: {}\nHuman: {}\nHuman key: {}\nCapabilities: {}\nExpires: {}\nCompare these values with the browser, then type AUTHORIZE followed by its code.\n",
        challenge.workspace_fingerprint,
        challenge.origin,
        challenge.human_id,
        challenge.human_public_key_fingerprint,
        capabilities,
        challenge.expires_at.to_rfc3339(),
    );
    if terminal.write_nonsecret(&prompt).is_err() {
        return Err(TerminalCeremonyError::TerminalUnavailable);
    }
    let mut echo = EchoGuard::disable(terminal)?;
    let confirmation = Zeroizing::new(match echo.read_confirmation(22, timeout) {
        Ok(value) => value,
        Err(error) => {
            echo.restore()?;
            return Err(error);
        }
    });
    echo.restore()?;

    let expected_code = Zeroizing::new(
        challenge_code(challenge).map_err(|_| TerminalCeremonyError::AuthorizationMismatch)?,
    );
    let expected = Zeroizing::new(format!("AUTHORIZE {}", expected_code.as_str()));
    let supplied = confirmation
        .strip_suffix("\r\n")
        .or_else(|| confirmation.strip_suffix('\n'))
        .ok_or(TerminalCeremonyError::AuthorizationMismatch)?;
    let matches = supplied.len() == expected.len()
        && bool::from(supplied.as_bytes().ct_eq(expected.as_bytes()));
    if !matches {
        return Err(TerminalCeremonyError::AuthorizationMismatch);
    }

    let attestation = signer.sign_challenge(challenge)?;
    authority
        .submit_attestation(attestation)
        .map_err(|error| match error {
            OperatorAuthError::ControlUnavailable => TerminalCeremonyError::TerminalUnavailable,
            _ => TerminalCeremonyError::SigningFailed,
        })?;
    attempt.complete();
    Ok(())
}

struct ChallengeAttempt<'a> {
    authority: &'a OperatorAuthAuthority,
    challenge_id: uuid::Uuid,
    complete: bool,
}

impl<'a> ChallengeAttempt<'a> {
    fn new(authority: &'a OperatorAuthAuthority, challenge_id: uuid::Uuid) -> Self {
        Self {
            authority,
            challenge_id,
            complete: false,
        }
    }

    fn complete(&mut self) {
        self.complete = true;
    }
}

impl Drop for ChallengeAttempt<'_> {
    fn drop(&mut self) {
        if !self.complete {
            let _ = self.authority.consume_failed_challenge(self.challenge_id);
        }
    }
}

struct EchoGuard<'a> {
    terminal: &'a mut dyn ControllingTerminal,
    active: bool,
}

impl<'a> EchoGuard<'a> {
    fn disable(terminal: &'a mut dyn ControllingTerminal) -> Result<Self, TerminalCeremonyError> {
        let mut guard = Self {
            terminal,
            active: true,
        };
        guard.terminal.set_echo(false)?;
        Ok(guard)
    }

    fn read_confirmation(
        &mut self,
        maximum_bytes: usize,
        timeout: Duration,
    ) -> Result<String, TerminalCeremonyError> {
        self.terminal.read_confirmation(maximum_bytes, timeout)
    }

    fn restore(&mut self) -> Result<(), TerminalCeremonyError> {
        match self.terminal.set_echo(true) {
            Ok(()) => {
                self.active = false;
                Ok(())
            }
            Err(_) => {
                let _ = self.terminal.set_echo(true);
                self.active = false;
                Err(TerminalCeremonyError::RestorationFailed)
            }
        }
    }
}

impl Drop for EchoGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.terminal.set_echo(true);
        }
    }
}

fn capability_label(capability: Capability) -> &'static str {
    match capability {
        Capability::ApprovalDecide => "approval.decide",
        Capability::ApprovalRead => "approval.read",
        Capability::AuditRead => "audit.read",
        Capability::RunCancel => "run.cancel",
        Capability::RunRead => "run.read",
        Capability::RunResume => "run.resume",
    }
}
