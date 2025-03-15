use std::{
    sync::{Arc, RwLock},
    task::Poll,
    time::Duration,
};

use anyhow::{bail, Result};
use tokio::{
    sync::{mpsc, Notify},
    task::JoinHandle,
    time,
};

pub struct EventSampling {
    pub buffer: Duration,
    pub timeout: Duration,
}

impl EventSampling {
    pub fn using<T>(self, rx: mpsc::Receiver<Poll<T>>) -> EventSampler<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        EventSampler::new(rx, self)
    }
}

#[derive(Debug, Clone)]
pub struct EventSampler<T> {
    state: Arc<RwLock<State<T>>>,
    event: Arc<Notify>,
}

#[derive(Debug, Clone)]
enum State<T> {
    Pending,
    Ready(T),
    Timeout,
}

impl<T: Clone + Send + Sync + 'static> EventSampler<T> {
    fn new(
        mut rx: mpsc::Receiver<Poll<T>>,
        EventSampling { buffer, timeout }: EventSampling,
    ) -> Self {
        let state = Arc::new(RwLock::new(State::Pending));
        let event = Arc::new(Notify::new());

        tokio::spawn({
            let state = state.clone();
            let event = event.clone();
            async move {
                let mut abort: Option<JoinHandle<()>> = None;
                while let Some(value) = time::timeout(timeout, rx.recv()).await.transpose() {
                    if let Some(abort) = abort.take() {
                        abort.abort();
                    }
                    match value {
                        Ok(Poll::Ready(value)) => {
                            let event = event.clone();
                            let state = state.clone();
                            abort = Some(tokio::spawn(async move {
                                time::sleep(buffer).await;
                                *state.write().unwrap() = State::Ready(value);
                                event.notify_waiters();
                            }));
                        }
                        Ok(Poll::Pending) => {
                            *state.write().unwrap() = State::Pending;
                            event.notify_waiters();
                        }
                        Err(_) => {
                            *state.write().unwrap() = State::Timeout;
                            event.notify_waiters();
                        }
                    }
                }
            }
        });

        Self { event, state }
    }

    pub async fn wait(&self) -> Result<T> {
        loop {
            {
                match self.state.read().unwrap().clone() {
                    State::Pending => {}
                    State::Ready(value) => return Ok(value),
                    State::Timeout => bail!("timed out waiting for ready event"),
                }
            }
            self.event.notified().await;
        }
    }
}
