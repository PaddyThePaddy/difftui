pub mod diff;
pub mod ui;

#[derive(Debug, thiserror::Error)]
pub enum DiffTuiError {
    #[error("io error {0}")]
    Io(#[from] std::io::Error),
    #[error("crossbeam_channel trying to send while channel is disconnected")]
    CrossbeamChannelSend,
    #[error("crossbeam_channel Receive error {0}")]
    CrossbeamChannelReceive(#[from] crossbeam::channel::RecvError),
    #[error("ignore error {0}")]
    IgnoreError(#[from] ignore::Error),
    #[error("Child thread panic")]
    ThreadPaniced,
    #[error("Tree node not found")]
    NodeNotFound,
}

impl<T> From<crossbeam::channel::SendError<T>> for DiffTuiError {
    fn from(_value: crossbeam::channel::SendError<T>) -> Self {
        Self::CrossbeamChannelSend
    }
}
