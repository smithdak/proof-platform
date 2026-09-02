use std::{
    sync::OnceLock,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use proof_kernel::{OperatorControlEnvironment, OperatorEnvironmentError, OperatorRandomPurpose};
use rand::{rngs::OsRng, RngCore};
use uuid::{Builder, Uuid};
use zeroize::Zeroizing;

/// Final nonconfigurable OS clock and entropy source used by the control process.
#[derive(Debug, Default)]
pub struct OsOperatorControlEnvironment {
    epoch: OnceLock<Instant>,
}

impl OsOperatorControlEnvironment {
    /// Constructs an environment without reading clock or entropy yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the current effective Unix user ID.
    #[cfg(target_os = "linux")]
    pub fn effective_user_id(&self) -> u32 {
        rustix::process::geteuid().as_raw()
    }
}

impl OperatorControlEnvironment for OsOperatorControlEnvironment {
    fn trusted_utc_now(&self) -> Result<DateTime<Utc>, OperatorEnvironmentError> {
        let now = SystemTime::now();
        now.duration_since(UNIX_EPOCH)
            .map_err(|_| OperatorEnvironmentError::ClockUnavailable)?;
        Ok(DateTime::<Utc>::from(now))
    }

    fn monotonic_millis(&self) -> Result<u64, OperatorEnvironmentError> {
        let epoch = self.epoch.get_or_init(Instant::now);
        u64::try_from(epoch.elapsed().as_millis())
            .map_err(|_| OperatorEnvironmentError::ClockUnavailable)
    }

    fn fill_random(
        &self,
        _purpose: OperatorRandomPurpose,
        output: &mut [u8],
    ) -> Result<(), OperatorEnvironmentError> {
        OsRng
            .try_fill_bytes(output)
            .map_err(|_| OperatorEnvironmentError::EntropyUnavailable)
    }

    fn new_uuid_v7(&self) -> Result<Uuid, OperatorEnvironmentError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| OperatorEnvironmentError::ClockUnavailable)?;
        let millis = u64::try_from(now.as_millis())
            .map_err(|_| OperatorEnvironmentError::ClockUnavailable)?;
        let mut random = Zeroizing::new([0_u8; 10]);
        self.fill_random(OperatorRandomPurpose::UuidEntropy, random.as_mut())?;
        Ok(Builder::from_unix_timestamp_millis(millis, &*random).into_uuid())
    }
}
