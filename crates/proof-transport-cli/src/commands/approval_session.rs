use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use rand::rngs::OsRng;
use rand::RngCore;
use tokio::sync::Notify;

pub(super) const BOOTSTRAP_TTL: Duration = Duration::from_secs(120);
pub(super) const SESSION_ABSOLUTE_TTL: Duration = Duration::from_secs(900);
pub(super) const SESSION_IDLE_TTL: Duration = Duration::from_secs(300);

pub(super) trait MonotonicClock: Send + Sync {
    fn now(&self) -> Duration;
}

#[derive(Debug)]
struct SystemClock(Instant);

impl SystemClock {
    fn new() -> Self {
        Self(Instant::now())
    }
}

impl MonotonicClock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BootstrapState {
    Absent,
    Pending {
        code: [u8; 8],
        deadline: Duration,
        exchange_claimed: bool,
    },
    Verified {
        code: [u8; 8],
        deadline: Duration,
        exchange_claimed: bool,
    },
    Exchanged,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionState {
    Absent,
    Active {
        secret: [u8; 32],
        instance_id: [u8; 16],
        workspace_binding: Arc<str>,
        absolute_deadline: Duration,
        idle_deadline: Duration,
    },
    Revoked,
    ExpiredAbsolute,
    ExpiredIdle,
}

struct AuthorityState {
    instance_id: [u8; 16],
    workspace_binding: Arc<str>,
    bootstrap: BootstrapState,
    session: SessionState,
}

pub(super) struct ApprovalAuthority {
    inner: Mutex<AuthorityState>,
    changed: Notify,
    clock: Arc<dyn MonotonicClock>,
}

pub(super) struct SessionLease<'a> {
    _guard: MutexGuard<'a, AuthorityState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalVerification {
    Verified,
    Rejected,
    Expired,
}

impl ApprovalAuthority {
    pub(super) fn new(workspace: &Path) -> Result<Arc<Self>> {
        let canonical = workspace.canonicalize().with_context(|| {
            format!(
                "could not bind approval UI workspace: {}",
                workspace.display()
            )
        })?;
        let workspace_binding = canonical
            .to_str()
            .context("approval UI workspace path must be valid UTF-8")?;
        let mut instance_id = [0_u8; 16];
        OsRng
            .try_fill_bytes(&mut instance_id)
            .context("could not create approval UI instance randomness")?;
        Ok(Self::with_parts(
            workspace_binding,
            instance_id,
            Arc::new(SystemClock::new()),
        ))
    }

    fn with_parts(
        workspace_binding: impl Into<Arc<str>>,
        instance_id: [u8; 16],
        clock: Arc<dyn MonotonicClock>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(AuthorityState {
                instance_id,
                workspace_binding: workspace_binding.into(),
                bootstrap: BootstrapState::Absent,
                session: SessionState::Absent,
            }),
            changed: Notify::new(),
            clock,
        })
    }

    fn lock_state(&self) -> Option<MutexGuard<'_, AuthorityState>> {
        match self.inner.lock() {
            Ok(state) => Some(state),
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                revoke_authority(&mut state);
                self.changed.notify_waiters();
                None
            }
        }
    }

    pub(super) fn register_bootstrap(&self, code: [u8; 8]) -> bool {
        let Some(mut state) = self.lock_state() else {
            return false;
        };
        if state.bootstrap != BootstrapState::Absent {
            return false;
        }
        state.bootstrap = BootstrapState::Pending {
            code,
            deadline: self.clock.now() + BOOTSTRAP_TTL,
            exchange_claimed: false,
        };
        drop(state);
        self.changed.notify_waiters();
        true
    }

    pub(super) async fn wait_for_pending(&self) -> Option<Duration> {
        loop {
            let notified = self.changed.notified();
            {
                let Some(mut state) = self.lock_state() else {
                    return None;
                };
                expire_bootstrap(&mut state, self.clock.now());
                match state.bootstrap {
                    BootstrapState::Pending { deadline, .. } => return Some(deadline),
                    BootstrapState::Absent => {}
                    _ => return None,
                }
            }
            notified.await;
        }
    }

    pub(super) fn verify_terminal(&self, candidate: Option<[u8; 8]>) -> TerminalVerification {
        let Some(mut state) = self.lock_state() else {
            return TerminalVerification::Rejected;
        };
        let now = self.clock.now();
        expire_bootstrap(&mut state, now);
        let result = match state.bootstrap {
            BootstrapState::Pending {
                code,
                deadline,
                exchange_claimed,
            } => {
                if candidate.is_some_and(|candidate| fixed_eq(&code, &candidate)) {
                    state.bootstrap = BootstrapState::Verified {
                        code,
                        deadline,
                        exchange_claimed,
                    };
                    TerminalVerification::Verified
                } else {
                    state.bootstrap = BootstrapState::Revoked;
                    TerminalVerification::Rejected
                }
            }
            BootstrapState::Expired => TerminalVerification::Expired,
            _ => TerminalVerification::Rejected,
        };
        drop(state);
        self.changed.notify_waiters();
        result
    }

    pub(super) fn expire_pending(&self) {
        if let Some(mut state) = self.lock_state() {
            expire_bootstrap(&mut state, self.clock.now());
        }
        self.changed.notify_waiters();
    }

    pub(super) async fn exchange(&self, code: [u8; 8]) -> Option<[u8; 32]> {
        {
            let Some(mut state) = self.lock_state() else {
                return None;
            };
            expire_bootstrap(&mut state, self.clock.now());
            match &mut state.bootstrap {
                BootstrapState::Pending {
                    code: expected,
                    exchange_claimed,
                    ..
                }
                | BootstrapState::Verified {
                    code: expected,
                    exchange_claimed,
                    ..
                } if fixed_eq(expected, &code) && !*exchange_claimed => {
                    *exchange_claimed = true;
                }
                _ => return None,
            }
        }

        loop {
            let notified = self.changed.notified();
            let wait = {
                let Some(mut state) = self.lock_state() else {
                    return None;
                };
                let now = self.clock.now();
                expire_bootstrap(&mut state, now);
                match state.bootstrap {
                    BootstrapState::Verified {
                        code: verified_code,
                        deadline,
                        exchange_claimed: true,
                    } if fixed_eq(&verified_code, &code) && now < deadline => {
                        let Some(secret) = new_session_secret(&state.instance_id, &code) else {
                            revoke_authority(&mut state);
                            drop(state);
                            self.changed.notify_waiters();
                            return None;
                        };
                        state.bootstrap = BootstrapState::Exchanged;
                        state.session = SessionState::Active {
                            secret,
                            instance_id: state.instance_id,
                            workspace_binding: state.workspace_binding.clone(),
                            absolute_deadline: now + SESSION_ABSOLUTE_TTL,
                            idle_deadline: now + SESSION_IDLE_TTL,
                        };
                        return Some(secret);
                    }
                    BootstrapState::Pending { deadline, .. } => deadline.saturating_sub(now),
                    _ => return None,
                }
            };
            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep(wait) => {
                    self.expire_pending();
                }
            }
        }
    }

    pub(super) fn session_lease(&self, candidate: &[u8; 32]) -> Option<SessionLease<'_>> {
        let mut guard = self.lock_state()?;
        let now = self.clock.now();
        expire_session(&mut guard, now);
        let (secret, instance_id, workspace_binding, absolute_deadline) = match &guard.session {
            SessionState::Active {
                secret,
                instance_id,
                workspace_binding,
                absolute_deadline,
                ..
            } => (
                *secret,
                *instance_id,
                workspace_binding.clone(),
                *absolute_deadline,
            ),
            _ => return None,
        };
        if instance_id != guard.instance_id
            || workspace_binding != guard.workspace_binding
            || !fixed_eq(&secret, candidate)
        {
            return None;
        }
        guard.session = SessionState::Active {
            secret,
            instance_id,
            workspace_binding,
            absolute_deadline,
            idle_deadline: (now + SESSION_IDLE_TTL).min(absolute_deadline),
        };
        Some(SessionLease { _guard: guard })
    }

    pub(super) fn revoke_session(&self, candidate: &[u8; 32]) -> bool {
        let Some(mut state) = self.lock_state() else {
            return false;
        };
        let now = self.clock.now();
        expire_session(&mut state, now);
        let valid = matches!(
            &state.session,
            SessionState::Active { secret, instance_id, workspace_binding, .. }
                if *instance_id == state.instance_id
                    && workspace_binding == &state.workspace_binding
                    && fixed_eq(secret, candidate)
        );
        if valid {
            state.session = SessionState::Revoked;
        }
        valid
    }

    pub(super) fn revoke_all(&self) {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        revoke_authority(&mut state);
        drop(state);
        self.changed.notify_waiters();
    }

    pub(super) fn now(&self) -> Duration {
        self.clock.now()
    }

    pub(super) fn terminal_attempt_active(&self) -> bool {
        let Some(mut state) = self.lock_state() else {
            return false;
        };
        expire_bootstrap(&mut state, self.clock.now());
        matches!(state.bootstrap, BootstrapState::Pending { .. })
    }

    #[cfg(test)]
    fn scope(&self) -> (Arc<str>, [u8; 16]) {
        let state = self.inner.lock().unwrap();
        (state.workspace_binding.clone(), state.instance_id)
    }

    #[cfg(test)]
    pub(super) fn test_active(workspace: &Path, secret: [u8; 32]) -> Arc<Self> {
        let authority = Self::with_parts(
            workspace.to_string_lossy().into_owned(),
            [0x22; 16],
            Arc::new(SystemClock::new()),
        );
        authority.inner.lock().unwrap().session = SessionState::Active {
            secret,
            instance_id: [0x22; 16],
            workspace_binding: workspace.to_string_lossy().into_owned().into(),
            absolute_deadline: SESSION_ABSOLUTE_TTL,
            idle_deadline: SESSION_IDLE_TTL,
        };
        authority
    }

    #[cfg(test)]
    pub(super) fn test_empty(workspace: &Path) -> Arc<Self> {
        Self::with_parts(
            workspace.to_string_lossy().into_owned(),
            [0x22; 16],
            Arc::new(SystemClock::new()),
        )
    }

    #[cfg(test)]
    pub(super) fn expire_session_for_test(&self) {
        let mut state = self.inner.lock().unwrap();
        if let SessionState::Active {
            absolute_deadline, ..
        } = &mut state.session
        {
            *absolute_deadline = self.clock.now();
        }
        expire_session(&mut state, self.clock.now());
    }
}

fn expire_bootstrap(state: &mut AuthorityState, now: Duration) {
    let deadline = match state.bootstrap {
        BootstrapState::Pending { deadline, .. } | BootstrapState::Verified { deadline, .. } => {
            Some(deadline)
        }
        _ => None,
    };
    if deadline.is_some_and(|deadline| now >= deadline) {
        state.bootstrap = BootstrapState::Expired;
    }
}

fn revoke_authority(state: &mut AuthorityState) {
    if matches!(
        state.bootstrap,
        BootstrapState::Absent | BootstrapState::Pending { .. } | BootstrapState::Verified { .. }
    ) {
        state.bootstrap = BootstrapState::Revoked;
    }
    if matches!(state.session, SessionState::Active { .. }) {
        state.session = SessionState::Revoked;
    }
}

fn expire_session(state: &mut AuthorityState, now: Duration) {
    let SessionState::Active {
        absolute_deadline,
        idle_deadline,
        ..
    } = &state.session
    else {
        return;
    };
    if now >= *absolute_deadline {
        state.session = SessionState::ExpiredAbsolute;
    } else if now >= *idle_deadline {
        state.session = SessionState::ExpiredIdle;
    }
}

fn new_session_secret(instance_id: &[u8; 16], code: &[u8; 8]) -> Option<[u8; 32]> {
    loop {
        let mut secret = [0_u8; 32];
        OsRng.try_fill_bytes(&mut secret).ok()?;
        if !fixed_eq(&secret[..16], instance_id) && !fixed_eq(&secret[..8], code) {
            return Some(secret);
        }
    }
}

pub(super) fn decode_lower_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != N * 2 || !value.is_ascii() {
        return None;
    }
    let mut decoded = [0_u8; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        decoded[index] = lower_nibble(pair[0])? << 4 | lower_nibble(pair[1])?;
    }
    Some(decoded)
}

fn lower_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn fixed_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let width = left.len().max(right.len());
    for index in 0..width {
        let left = left.get(index).copied().unwrap_or(0);
        let right = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(left ^ right);
    }
    difference == 0
}

pub(super) fn parse_terminal_code(value: &[u8]) -> Option<[u8; 8]> {
    let valid_layout = match value.len() {
        16 => value.iter().all(u8::is_ascii_hexdigit),
        19 => value.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 9 | 14) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        }),
        _ => false,
    };
    if !valid_layout {
        return None;
    }
    let mut canonical = [0_u8; 16];
    let mut length = 0;
    for byte in value.iter().copied() {
        if byte == b'-' {
            continue;
        }
        canonical[length] = byte.to_ascii_lowercase();
        length += 1;
    }
    decode_lower_hex(std::str::from_utf8(&canonical).ok()?)
}

#[cfg(target_os = "linux")]
pub(super) struct ControllingTerminal {
    file: Mutex<File>,
    original: rustix::termios::Termios,
}

#[cfg(target_os = "linux")]
impl ControllingTerminal {
    pub(super) fn open() -> Result<Arc<Self>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("approval UI requires a controlling terminal")?;
        if !rustix::termios::isatty(&file) {
            bail!("approval UI requires a controlling terminal");
        }
        let original = rustix::termios::tcgetattr(&file)
            .context("could not read controlling-terminal attributes")?;
        let terminal = Arc::new(Self {
            file: Mutex::new(file),
            original,
        });
        terminal.preflight_non_echo()?;
        Ok(terminal)
    }

    fn preflight_non_echo(&self) -> Result<()> {
        let file = self
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal lock failed"))?;
        let protected = non_echo_mode(&self.original);
        rustix::termios::tcsetattr(&*file, rustix::termios::OptionalActions::Now, &protected)
            .context("could not establish non-echoing terminal input")?;
        rustix::termios::tcsetattr(
            &*file,
            rustix::termios::OptionalActions::Now,
            &self.original,
        )
        .context("could not restore terminal after non-echo preflight")?;
        verify_terminal_attributes(&file, &self.original)
    }

    pub(super) fn restore_verified(&self) -> Result<()> {
        let file = self
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal lock failed"))?;
        rustix::termios::tcsetattr(
            &*file,
            rustix::termios::OptionalActions::Now,
            &self.original,
        )
        .context("could not restore controlling-terminal echo")?;
        verify_terminal_attributes(&file, &self.original)
    }

    pub(super) fn read_attempt(
        self: &Arc<Self>,
        authority: &ApprovalAuthority,
        deadline: Duration,
    ) -> Result<Option<[u8; 8]>> {
        let mut guard = TerminalModeGuard::enter(self.clone())?;
        let result = (|| -> Result<Option<[u8; 8]>> {
            guard.write_all(b"Enter the code displayed in the approval page: ")?;
            guard.flush()?;
            let mut input = Vec::with_capacity(20);
            let mut byte = [0_u8; 1];
            loop {
                if authority.now() >= deadline || !authority.terminal_attempt_active() {
                    break Ok(None);
                }
                match guard.read(&mut byte) {
                    Ok(0) => continue,
                    Ok(_) => match byte[0] {
                        b'\r' | b'\n' => break Ok(parse_terminal_code(&input)),
                        value if terminal_input_is_eof(value) => break Ok(None),
                        3 => bail!("approval UI terminal confirmation interrupted"),
                        8 | 127 => {
                            input.pop();
                        }
                        value if input.len() < 64 => input.push(value),
                        _ => break Ok(None),
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => break Err(error).context("controlling-terminal input failed"),
                }
            }
        })();
        guard.restore_verified()?;
        result
    }

    pub(super) fn write_status(&self, status: &str) -> Result<()> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal lock failed"))?;
        writeln!(file, "\n{status}").context("could not write controlling-terminal status")?;
        file.flush()
            .context("could not flush controlling-terminal status")
    }
}

#[cfg(target_os = "linux")]
impl Drop for ControllingTerminal {
    fn drop(&mut self) {
        if let Ok(file) = self.file.get_mut() {
            let _ = rustix::termios::tcsetattr(
                file,
                rustix::termios::OptionalActions::Now,
                &self.original,
            );
        }
    }
}

#[cfg(target_os = "linux")]
struct TerminalModeGuard {
    terminal: Arc<ControllingTerminal>,
    file: File,
    restored: bool,
}

#[cfg(target_os = "linux")]
impl TerminalModeGuard {
    fn enter(terminal: Arc<ControllingTerminal>) -> Result<Self> {
        let file = terminal
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal lock failed"))?
            .try_clone()
            .context("could not duplicate controlling terminal")?;
        let protected = non_echo_mode(&terminal.original);
        rustix::termios::tcsetattr(&file, rustix::termios::OptionalActions::Now, &protected)
            .context("could not disable controlling-terminal echo")?;
        Ok(Self {
            terminal,
            file,
            restored: false,
        })
    }

    fn restore_verified(&mut self) -> Result<()> {
        self.terminal.restore_verified()?;
        self.restored = true;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn non_echo_mode(original: &rustix::termios::Termios) -> rustix::termios::Termios {
    let mut protected = original.clone();
    protected.local_modes.remove(
        rustix::termios::LocalModes::ECHO
            | rustix::termios::LocalModes::ECHONL
            | rustix::termios::LocalModes::ICANON,
    );
    protected.special_codes[rustix::termios::SpecialCodeIndex::VMIN] = 0;
    protected.special_codes[rustix::termios::SpecialCodeIndex::VTIME] = 1;
    protected
}

#[cfg(target_os = "linux")]
fn terminal_input_is_eof(byte: u8) -> bool {
    byte == 0x04
}

#[cfg(target_os = "linux")]
impl Read for TerminalModeGuard {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buffer)
    }
}

#[cfg(target_os = "linux")]
impl Write for TerminalModeGuard {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.file.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

#[cfg(target_os = "linux")]
impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.terminal.restore_verified();
        }
    }
}

#[cfg(target_os = "linux")]
fn verify_terminal_attributes(file: &File, expected: &rustix::termios::Termios) -> Result<()> {
    let actual = rustix::termios::tcgetattr(file)
        .context("could not verify controlling-terminal restoration")?;
    if actual.input_modes != expected.input_modes
        || actual.output_modes != expected.output_modes
        || actual.control_modes != expected.control_modes
        || actual.local_modes != expected.local_modes
        || actual.line_discipline != expected.line_discipline
        || actual.input_speed() != expected.input_speed()
        || actual.output_speed() != expected.output_speed()
        || format!("{:?}", actual.special_codes) != format!("{:?}", expected.special_codes)
    {
        bail!("controlling-terminal restoration could not be verified");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct ManualClock(AtomicU64);

    impl ManualClock {
        fn new() -> Arc<Self> {
            Arc::new(Self(AtomicU64::new(0)))
        }

        fn set(&self, seconds: u64) {
            self.0.store(seconds, Ordering::SeqCst);
        }
    }

    impl MonotonicClock for ManualClock {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.load(Ordering::SeqCst))
        }
    }

    fn new_authority(clock: Arc<ManualClock>) -> Arc<ApprovalAuthority> {
        ApprovalAuthority::with_parts("/workspace/a", [7; 16], clock)
    }

    async fn establish(authority: &Arc<ApprovalAuthority>, code: [u8; 8]) -> [u8; 32] {
        assert!(authority.register_bootstrap(code));
        let exchange = {
            let authority = authority.clone();
            tokio::spawn(async move { authority.exchange(code).await })
        };
        tokio::task::yield_now().await;
        assert_eq!(
            authority.verify_terminal(Some(code)),
            TerminalVerification::Verified
        );
        exchange.await.unwrap().unwrap()
    }

    #[test]
    fn strict_wire_and_terminal_code_parsing() {
        assert_eq!(
            decode_lower_hex::<8>("0123456789abcdef"),
            Some([1, 35, 69, 103, 137, 171, 205, 239])
        );
        assert!(decode_lower_hex::<8>("0123456789ABCDEF").is_none());
        assert!(decode_lower_hex::<8>("0123").is_none());
        assert_eq!(
            parse_terminal_code(b"0123-4567-89AB-CDEF"),
            Some([1, 35, 69, 103, 137, 171, 205, 239])
        );
        assert_eq!(
            parse_terminal_code(b"0123456789ABCDEF"),
            Some([1, 35, 69, 103, 137, 171, 205, 239])
        );
        assert!(parse_terminal_code(b"0123-4567-89ab-cdeg").is_none());
        assert!(parse_terminal_code(b"0123 4567 89ab cdef").is_none());
        assert!(parse_terminal_code(b"01234-567-89ab-cdef").is_none());
        assert!(parse_terminal_code(b"0123--4567-89ab-cdef").is_none());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn terminal_eot_is_authority_loss_and_restoration_is_explicit() {
        assert!(terminal_input_is_eof(0x04));
        assert!(!terminal_input_is_eof(b'4'));
        let source = include_str!("approval_session.rs");
        assert!(source.contains("guard.restore_verified()?;\n        result"));
        assert!(source.contains("impl Drop for ControllingTerminal"));
    }

    #[test]
    fn mismatch_is_terminal_and_deadline_equality_expires() {
        let clock = ManualClock::new();
        let authority = new_authority(clock.clone());
        assert!(authority.register_bootstrap([1; 8]));
        assert_eq!(
            authority.verify_terminal(Some([2; 8])),
            TerminalVerification::Rejected
        );
        assert_eq!(
            authority.verify_terminal(Some([1; 8])),
            TerminalVerification::Rejected
        );

        let authority = new_authority(clock.clone());
        assert!(authority.register_bootstrap([1; 8]));
        clock.set(120);
        assert_eq!(
            authority.verify_terminal(Some([1; 8])),
            TerminalVerification::Expired
        );
    }

    #[tokio::test]
    async fn exchange_waits_for_verification_and_is_single_use() {
        let clock = ManualClock::new();
        let authority = new_authority(clock);
        let code = [3; 8];
        assert!(authority.register_bootstrap(code));
        let pending = {
            let authority = authority.clone();
            tokio::spawn(async move { authority.exchange(code).await })
        };
        tokio::task::yield_now().await;
        assert!(!pending.is_finished());
        assert_eq!(
            authority.verify_terminal(Some(code)),
            TerminalVerification::Verified
        );
        let secret = pending.await.unwrap().unwrap();
        assert_ne!(&secret[..8], &code);
        assert!(authority.exchange(code).await.is_none());
        assert!(authority.session_lease(&secret).is_some());
    }

    #[tokio::test]
    async fn verify_before_exchange_preserves_the_unclaimed_exchange() {
        let clock = ManualClock::new();
        let authority = new_authority(clock);
        let code = [0x35; 8];
        assert!(authority.register_bootstrap(code));
        assert_eq!(
            authority.verify_terminal(Some(code)),
            TerminalVerification::Verified
        );
        let secret = authority.exchange(code).await.unwrap();
        assert!(authority.session_lease(&secret).is_some());
        assert!(authority.exchange(code).await.is_none());
    }

    #[tokio::test]
    async fn shutdown_terminalizes_and_wakes_an_absent_bootstrap_waiter() {
        let clock = ManualClock::new();
        let authority = new_authority(clock);
        let waiter = {
            let authority = authority.clone();
            tokio::spawn(async move { authority.wait_for_pending().await })
        };
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        authority.revoke_all();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), waiter)
                .await
                .expect("absent waiter hung during shutdown")
                .unwrap(),
            None
        );
        assert!(!authority.register_bootstrap([0x51; 8]));
    }

    #[tokio::test]
    async fn concurrent_exchange_has_one_deterministic_claimed_winner() {
        let clock = ManualClock::new();
        let authority = new_authority(clock);
        let code = [4; 8];
        assert!(authority.register_bootstrap(code));
        let first = {
            let authority = authority.clone();
            tokio::spawn(async move { authority.exchange(code).await })
        };
        tokio::task::yield_now().await;
        let second = authority.exchange(code).await;
        assert!(second.is_none());
        assert_eq!(
            authority.verify_terminal(Some(code)),
            TerminalVerification::Verified
        );
        assert!(first.await.unwrap().is_some());
    }

    #[tokio::test]
    async fn terminal_loss_expiry_and_cross_instance_wake_without_session() {
        let clock = ManualClock::new();
        let authority = new_authority(clock.clone());
        let code = [0x44; 8];
        assert!(authority.register_bootstrap(code));
        let exchange = {
            let authority = authority.clone();
            tokio::spawn(async move { authority.exchange(code).await })
        };
        tokio::task::yield_now().await;
        assert_eq!(
            authority.verify_terminal(None),
            TerminalVerification::Rejected
        );
        assert!(exchange.await.unwrap().is_none());

        let authority = new_authority(clock.clone());
        assert!(authority.register_bootstrap(code));
        let exchange = {
            let authority = authority.clone();
            tokio::spawn(async move { authority.exchange(code).await })
        };
        tokio::task::yield_now().await;
        clock.set(120);
        authority.expire_pending();
        assert!(exchange.await.unwrap().is_none());

        let other = ApprovalAuthority::with_parts("/workspace/a", [8; 16], clock);
        assert_eq!(
            other.verify_terminal(Some(code)),
            TerminalVerification::Rejected
        );
        assert!(other.exchange(code).await.is_none());
    }

    #[tokio::test]
    async fn lost_response_and_cross_scope_credentials_fail_closed() {
        let clock = ManualClock::new();
        let authority = new_authority(clock.clone());
        let code = [5; 8];
        let secret = establish(&authority, code).await;
        assert!(authority.exchange(code).await.is_none());

        let other_instance = ApprovalAuthority::with_parts("/workspace/a", [8; 16], clock.clone());
        let other_workspace = ApprovalAuthority::with_parts("/workspace/b", [7; 16], clock);
        assert_ne!(authority.scope(), other_instance.scope());
        assert_ne!(authority.scope(), other_workspace.scope());
        assert!(other_instance.session_lease(&secret).is_none());
        assert!(other_workspace.session_lease(&secret).is_none());
    }

    #[tokio::test]
    async fn session_idle_absolute_expiry_and_revoke_are_fail_closed() {
        let clock = ManualClock::new();
        let authority = new_authority(clock.clone());
        let code = [6; 8];
        let secret = establish(&authority, code).await;

        clock.set(299);
        assert!(authority.session_lease(&secret).is_some());
        clock.set(599);
        assert!(authority.session_lease(&secret).is_none());

        let clock = ManualClock::new();
        let authority = new_authority(clock.clone());
        let secret = establish(&authority, code).await;
        clock.set(900);
        assert!(authority.session_lease(&secret).is_none());

        let clock = ManualClock::new();
        let authority = new_authority(clock);
        let secret = establish(&authority, code).await;
        assert!(authority.revoke_session(&secret));
        assert!(authority.session_lease(&secret).is_none());
    }

    #[tokio::test]
    async fn mutation_lease_linearizes_revoke_and_expiry() {
        let clock = ManualClock::new();
        let authority = new_authority(clock.clone());
        let secret = establish(&authority, [9; 8]).await;
        let lease = authority.session_lease(&secret).unwrap();

        std::thread::scope(|scope| {
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (result_tx, result_rx) = std::sync::mpsc::channel();
            let authority = authority.clone();
            scope.spawn(move || {
                started_tx.send(()).unwrap();
                result_tx.send(authority.revoke_session(&secret)).unwrap();
            });
            started_rx.recv().unwrap();
            assert!(result_rx.try_recv().is_err());
            drop(lease);
            assert!(result_rx.recv().unwrap());
        });
        assert!(authority.session_lease(&secret).is_none());

        let clock = ManualClock::new();
        let authority = new_authority(clock.clone());
        let secret = establish(&authority, [10; 8]).await;
        let lease = authority.session_lease(&secret).unwrap();
        clock.set(900);
        // Work that acquired the lease while valid may finish before expiry linearizes.
        drop(lease);
        assert!(authority.session_lease(&secret).is_none());
    }

    #[tokio::test]
    async fn poisoned_authority_erases_active_credentials_and_fails_closed() {
        let clock = ManualClock::new();
        let authority = new_authority(clock);
        let secret = establish(&authority, [0x55; 8]).await;
        let poisoned = authority.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = poisoned.inner.lock().unwrap();
            panic!("synthetic authority failure");
        }));

        assert!(authority.session_lease(&secret).is_none());
        let state = match authority.inner.lock() {
            Ok(_) => panic!("authority mutex unexpectedly recovered"),
            Err(poisoned) => poisoned.into_inner(),
        };
        assert_eq!(state.session, SessionState::Revoked);
    }
}
