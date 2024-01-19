use std::collections::HashMap;
use std::ops::DerefMut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use reqwest_eventsource::{Event as SseEvent, EventSource};
use serde::{Deserialize, Serialize};
use futures_util::StreamExt;
use crate::errors::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    commit_id: String,
    content: serde_json::Value,
    metadata: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BatchCreationParameters {
    pub client_id: String,
}

struct BatchCreationRequest {
    params: BatchCreationParameters,
    tx: tokio::sync::oneshot::Sender<Result<(), Error>>
}

pub type EventResponder = tokio::sync::oneshot::Sender<Result<Vec<Event>, Error>>;

struct GetRequest {
    limit: Option<usize>,
    tx: EventResponder,
}

struct RunningEventSource {
    rx: tokio::sync::mpsc::Receiver<Result<Event, Error>>,
    shutdown: Arc<AtomicBool>,
}

#[derive(Default)]
struct PendingResults {
    events: Vec<Event>,
    error: Option<Error>,
}

#[derive(Clone)]
pub struct Batcher {
    stream_create_requests: Arc<tokio::sync::Mutex<HashMap<String, BatchCreationRequest>>>,
    stream_get_requests: Arc<tokio::sync::Mutex<HashMap<String, GetRequest>>>,
}

impl Batcher {
    pub fn new() -> Self {
        let me = Self {
            stream_create_requests: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            stream_get_requests: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        };
        tokio::spawn(me.clone().run());
        me
    }

    pub async fn create_batcher(&self, params: BatchCreationParameters) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.stream_create_requests.lock().await.insert(
            params.client_id.clone(),
            BatchCreationRequest {
                params,
                tx,
            }
        );
        let res = rx.await.map_err(Error::Recv)?;
        res
    }

    pub async fn get_batch(&self, client_id: &str, limit: Option<usize>) -> Result<Vec<Event>, Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.stream_get_requests.lock().await.insert(
            client_id.to_string(),
            GetRequest {
                limit,
                tx,
            }
        );
        match rx.await {
            Err(_) => Err(Error::custom("Failed to receive batch")),
            Ok(r) => r,
        }
    }

    pub async fn run(self) {
        let mut streams: HashMap<String, RunningEventSource> = HashMap::default();
        let mut outstanding_events: HashMap<String, PendingResults> = HashMap::default();
        loop {
            let mut streams_to_process = std::mem::replace(&mut streams, HashMap::default());
            for(k, mut batcher) in streams_to_process.drain() {
                let entry = outstanding_events.entry(k.clone()).or_insert_with(|| PendingResults::default());

                loop {
                    match batcher.rx.try_recv() {
                        Ok(Ok(event)) => {
                            entry.events.push(event);
                        }
                        Ok(Err(e)) => {
                            entry.error = Some(e);
                            break;
                        }
                        Err(e) => {
                            if let tokio::sync::mpsc::error::TryRecvError::Empty = e {
                                streams.insert(k, batcher);
                            }
                            break;
                        }
                    }
                }
            }
            let create_requests = {
                let mut m = self.stream_create_requests.lock().await;
                std::mem::replace(m.deref_mut(), HashMap::default())
            };
            for (client_id, req) in create_requests.into_iter() {
                let (tx, rx) = tokio::sync::mpsc::channel(100);
                let shutdown = Arc::new(AtomicBool::new(false));
                tokio::spawn(run_event_source(req.params, shutdown.clone(), tx));
                streams.insert(client_id, RunningEventSource {
                    rx,
                    shutdown
                });
                req.tx.send(Ok(())).unwrap();
            }
            let get_requests = {
                let mut m = self.stream_get_requests.lock().await;
                std::mem::replace(m.deref_mut(), HashMap::default())
            };
            for (client_id, req) in get_requests.into_iter() {
                if let Some(mut results) = outstanding_events.remove(&client_id) {
                    if results.events.is_empty() {
                        if results.error.is_some() {
                            let err = results.error.take().unwrap();
                            if let Err(_) = req.tx.send(Err(err)) {
                                tracing::debug!("Failed to send error to client");
                            }
                        } else {
                            if let Err(_) = req.tx.send(Ok(Vec::new())) {
                                tracing::debug!("Failed to send empty results to client");
                            }
                        }
                    } else {
                        let split_idx = std::cmp::min(results.events.len(),
                            req.limit.unwrap_or(usize::max_value())
                        );
                        let mut rem = results.events.split_off(split_idx);
                        std::mem::swap(&mut results.events, &mut rem);
                        if let Err(_) = req.tx.send(Ok(rem)) {
                            tracing::debug!("Failed to send results to client");
                        }
                    }
                } else {
                    if let Err(_) = req.tx.send(Err(Error::NotFound(client_id))) {
                        tracing::debug!("Failed to send not found to client");
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}

fn event_source_for_params(_params: &BatchCreationParameters) -> EventSource {
    EventSource::get(&format!("http://localhost:7007/api/v0/feed/aggregation/documents"))
}

async fn run_event_source(
    params: BatchCreationParameters,
    shutdown: Arc<AtomicBool>,
    tx: tokio::sync::mpsc::Sender<Result<Event, Error>>
) {
    let mut es = event_source_for_params(&params);
    while !shutdown.load(Ordering::Relaxed) {
        match es.next().await {
            Some(Ok(res)) => {
                if let SseEvent::Message(msg) = res {
                    match serde_json::from_str::<Event>(&msg.data) {
                        Ok(event) => {
                            if let Err(_) = tx.send(Ok(event)).await {
                                return;
                            }
                        }
                        Err(e) => {
                            if let Err(_) = tx.send(Err(Error::Json(e))).await {
                                return;
                            }
                        }
                    }
                }
            }
            _ => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                es = event_source_for_params(&params);
            }
        }
    }
}