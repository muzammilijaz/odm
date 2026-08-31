use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// Cooperative pause primitive. Unlike cancellation, pausing keeps sockets
/// open and just blocks between reads, so resuming is instant.
#[derive(Clone)]
pub struct PauseController {
    tx: watch::Sender<bool>,
}

#[derive(Clone)]
pub struct PauseToken {
    rx: watch::Receiver<bool>,
}

impl PauseController {
    pub fn new() -> (Self, PauseToken) {
        let (tx, rx) = watch::channel(false);
        (Self { tx }, PauseToken { rx })
    }

    pub fn pause(&self) {
        let _ = self.tx.send(true);
    }

    pub fn resume(&self) {
        let _ = self.tx.send(false);
    }

    pub fn is_paused(&self) -> bool {
        *self.tx.borrow()
    }
}

impl PauseToken {
    pub async fn wait_while_paused(&mut self) {
        loop {
            if !*self.rx.borrow() {
                return;
            }
            if self.rx.changed().await.is_err() {
                return;
            }
        }
    }
}

/// Bundles the cancel + pause handles a caller uses to control a running download.
#[derive(Clone)]
pub struct DownloadControl {
    pub cancel: CancellationToken,
    pub pause: PauseController,
}

impl DownloadControl {
    pub fn new() -> (Self, PauseToken) {
        let (pause, pause_token) = PauseController::new();
        (
            Self {
                cancel: CancellationToken::new(),
                pause,
            },
            pause_token,
        )
    }
}
