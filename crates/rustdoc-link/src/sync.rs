use std::{
    sync::{Arc, RwLock},
    task::Poll,
    time::Duration,
};

use tokio::{
    sync::{mpsc, Notify},
    task::JoinHandle,
    time::sleep,
};

#[derive(Debug, Clone)]
pub struct Subsample<T> {
    state: Arc<RwLock<Poll<T>>>,
    event: Arc<Notify>,
}

impl<T: Clone + Send + Sync + 'static> Subsample<T> {
    pub fn new(mut rx: mpsc::Receiver<Poll<T>>, wait: Duration) -> Self {
        let state = Arc::new(RwLock::new(Poll::Pending));
        let event = Arc::new(Notify::new());

        tokio::spawn({
            let state = state.clone();
            let event = event.clone();
            async move {
                let mut abort: Option<JoinHandle<()>> = None;
                while let Some(value) = rx.recv().await {
                    if let Some(abort) = abort.take() {
                        abort.abort();
                    }
                    if value.is_ready() {
                        let event = event.clone();
                        let state = state.clone();
                        abort = Some(tokio::spawn(async move {
                            sleep(wait).await;
                            *state.write().unwrap() = value;
                            event.notify_waiters();
                        }));
                    } else {
                        *state.write().unwrap() = value;
                    }
                }
            }
        });

        Self { event, state }
    }

    pub async fn wait(&self) -> T {
        loop {
            {
                if let Poll::Ready(value) = self.state.read().unwrap().clone() {
                    return value;
                }
            }
            self.event.notified().await;
        }
    }
}
