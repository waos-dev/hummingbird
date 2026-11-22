use super::*;
use tokio::sync::mpsc::UnboundedSender;
use tracing::warn;

/// With no mount table to read, the filesystem is one mount at `/`, so a root is available while
/// it exists. WaOS port: a WaOS program's files are one tree, every folder the user picked
/// mounted in it at its own path.
pub(super) fn current_mounts(_roots: &[PathBuf]) -> MountSnapshot {
    MountSnapshot::new([PathBuf::from("/")])
}

pub(super) fn start_monitor(_roots: Vec<PathBuf>, _tx: UnboundedSender<()>) {
    warn!("No native mount monitor is available on this platform");
}
