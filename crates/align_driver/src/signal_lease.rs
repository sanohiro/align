use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

static DRIVER_SIGNAL_OWNER: AtomicBool = AtomicBool::new(false);

/// Process-global lease shared by driver modes that replace graceful-signal
/// dispositions.
pub(super) struct DriverSignalLease {
    owned: bool,
}

impl DriverSignalLease {
    pub(super) fn acquire() -> io::Result<Self> {
        DRIVER_SIGNAL_OWNER
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| io::Error::other("another driver signal owner is active"))?;
        Ok(Self { owned: true })
    }

    fn release_owned(&mut self) {
        if self.owned {
            DRIVER_SIGNAL_OWNER.store(false, Ordering::Release);
            self.owned = false;
        }
    }
}

impl Drop for DriverSignalLease {
    fn drop(&mut self) {
        self.release_owned();
    }
}
