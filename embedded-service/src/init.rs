//! Code related to initialization states and ordering

use embassy_sync::once_lock::OnceLock;

static REGISTRATION_DONE: OnceLock<()> = OnceLock::new();

/// Wait for registration to complete
pub async fn wait_for_registration() {
    REGISTRATION_DONE.get().await;
}

/// Signal that registration is done
pub fn registration_done() {
    REGISTRATION_DONE.get_or_init(|| ());
}
