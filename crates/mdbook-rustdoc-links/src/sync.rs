use std::{
    sync::{Arc, RwLock},
    task::Poll,
    time::Duration,
};

use anyhow::{Result, bail};
use tokio::{
    sync::{Notify, mpsc},
    task::JoinHandle,
    time,
};
use tracing::{Instrument, trace, trace_span};

use mdbookkit::error::ExpectLock;

pub struct Debouncing<T> {
    pub debounce: Duration,
    pub timeout: Duration,
    pub receiver: mpsc::Receiver<Poll<T>>,
}

impl<T> Debouncing<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn build(self) -> Debounce<T> {
        let state = Arc::new(RwLock::new(State::Pending));
        let event = Arc::new(Notify::new());

        let span = trace_span!("debounce", ?self);

        let Self {
            debounce,
            timeout,
            mut receiver,
        } = self;

        tokio::spawn({
            trace!("spawning debounce thread");

            let state = state.clone();
            let event = event.clone();
            let trace = span.id();

            async move {
                let mut abort: Option<JoinHandle<()>> = None;

                while let Some(value) = time::timeout(timeout, receiver.recv()).await.transpose() {
                    trace!("received new event");

                    if let Some(abort) = abort.take() {
                        trace!("canceling deferred notification");
                        abort.abort();
                    }

                    match value {
                        Ok(Poll::Ready(value)) => {
                            trace!("state is ready; deferring notification");
                            let event = event.clone();
                            let state = state.clone();
                            let trace = trace.clone();
                            abort = Some(tokio::spawn(async move {
                                time::sleep(debounce).await;
                                *state.write().expect_lock() = State::Ready(value);
                                trace!(parent: trace, "state is ready; notifying");
                                event.notify_waiters();
                            }));
                        }

                        Ok(Poll::Pending) => {
                            trace!("state is pending");
                            *state.write().expect_lock() = State::Pending;
                            event.notify_waiters();
                        }

                        Err(_) => {
                            trace!("timed out waiting for state to become ready");
                            *state.write().expect_lock() = State::Timeout;
                            event.notify_waiters();
                        }
                    }
                }
            }
            .instrument(span)
        });

        Debounce { event, state }
    }
}

/// Some kind of [debouncing].
///
/// Listens to events over an [`mpsc::Receiver<Poll<T>>`] and [notifies][Notify]
/// subscribers of [`Poll::Ready`], but only if they are not "immediately"
/// followed by more [`Poll::Pending`], the timing of which is determined by a
/// configured [buffering time][EventSampling::buffer].
///
/// [debouncing]: https://developer.mozilla.org/en-US/docs/Glossary/Debounce
#[derive(Debug, Clone)]
pub struct Debounce<T> {
    state: Arc<RwLock<State<T>>>,
    event: Arc<Notify>,
}

#[derive(Debug, Clone)]
enum State<T> {
    Pending,
    Ready(T),
    Timeout,
}

impl<T: Clone + Send + Sync + 'static> Debounce<T> {
    pub async fn wait(&self) -> Result<T> {
        loop {
            {
                match self.state.read().expect_lock().clone() {
                    State::Pending => {}
                    State::Ready(value) => {
                        return Ok(value);
                    }
                    State::Timeout => {
                        bail!("Timed out waiting for ready event")
                    }
                }
            }
            self.event.notified().await;
        }
    }
}

impl<T> std::fmt::Debug for Debouncing<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            debounce,
            timeout,
            receiver: _,
        } = &self;
        f.debug_struct("Debouncing")
            .field("debounce", &debounce)
            .field("timeout", &timeout)
            .finish_non_exhaustive()
    }
}
