use crate::errors::Error;
use futures_util::StreamExt;
use reqwest_eventsource::{Event as SseEvent, EventSource as ReqwestEventSource};
use schema::Event;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

pub struct EventSource {
    client_id: String,
    url: Url,
    inner: ReqwestEventSource,
}

pub struct RunningEventSource {
    pub shutdown: Arc<AtomicBool>,
    pub rx: tokio::sync::mpsc::Receiver<Result<Event, Error>>,
    pub join: actix_web::rt::task::JoinHandle<()>,
}

impl EventSource {
    pub fn new(client_id: &str, ceramic_url: &Url) -> EventSource {
        let ceramic_url = ceramic_url
            .join("/api/v0/feed/aggregation/documents")
            .unwrap();
        tracing::debug!(
            "Creating event source against {} with client {}",
            ceramic_url,
            client_id
        );
        Self {
            client_id: client_id.to_string(),
            url: ceramic_url.clone(),
            inner: ReqwestEventSource::get(ceramic_url.to_string()),
        }
    }

    pub fn run(self) -> RunningEventSource {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let shutdown = Arc::new(AtomicBool::new(false));
        let join = tokio::spawn(run(self, tx, shutdown.clone()));
        RunningEventSource { shutdown, rx, join }
    }
}

async fn run(
    mut es: EventSource,
    tx: tokio::sync::mpsc::Sender<Result<Event, Error>>,
    shutdown: Arc<AtomicBool>,
) {
    tracing::debug!("Running event source for client {}", es.client_id);
    while !shutdown.load(Ordering::Relaxed) {
        match es.inner.next().await {
            Some(Ok(res)) => {
                if let SseEvent::Message(msg) = res {
                    match serde_json::from_str::<Event>(&msg.data) {
                        Ok(event) => {
                            if tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            if tx.send(Err(Error::Json(e))).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
            r => {
                if let Some(Err(e)) = r {
                    tracing::warn!("Event source for client {} failed: {:?}", es.client_id, e);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                tracing::info!("Reforming event source for client {}", es.client_id);
                es = EventSource::new(&es.client_id, &es.url);
            }
        }
    }
}
