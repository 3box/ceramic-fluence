use crate::errors::Error;
use crate::persistence::Persistence;
use futures_util::StreamExt;
use reqwest_eventsource::{Event as SseEvent, EventSource};
use schema::Event;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::ops::DerefMut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug, Deserialize)]
pub struct BatchCreationParameters {
    pub client_id: String,
}

struct BatchCreationRequest {
    params: BatchCreationParameters,
    tx: tokio::sync::oneshot::Sender<Result<(), Error>>,
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
    db: Arc<dyn Persistence + Send + Sync>,
    ceramic_url: Url,
    stream_create_requests: Arc<tokio::sync::Mutex<HashMap<String, BatchCreationRequest>>>,
    stream_get_requests: Arc<tokio::sync::Mutex<HashMap<String, GetRequest>>>,
    stream_delete_requests: Arc<tokio::sync::Mutex<HashSet<String>>>,
}

impl Batcher {
    pub fn new(db: Arc<dyn Persistence + Send + Sync>) -> Result<Self, Error> {
        let u = std::env::var("CERAMIC_URL").expect("CERAMIC_URL not set in environment");
        let u = Url::parse(&u)?;
        let u = u.join("/api/v0/feed/aggregation/documents")?;
        Ok(Self::new_with_url(db, u))
    }

    pub fn new_with_url(db: Arc<dyn Persistence + Send + Sync>, ceramic_url: Url) -> Self {
        let me = Self {
            db,
            ceramic_url,
            stream_create_requests: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            stream_get_requests: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            stream_delete_requests: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
        };
        tokio::spawn(me.clone().run());
        me
    }

    pub async fn create_batcher(&self, params: BatchCreationParameters) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.stream_create_requests.lock().await.insert(
            params.client_id.clone(),
            BatchCreationRequest { params, tx },
        );

        rx.await.map_err(Error::Recv)?
    }

    pub async fn get_batch(
        &self,
        client_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<Event>, Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.stream_get_requests
            .lock()
            .await
            .insert(client_id.to_string(), GetRequest { limit, tx });
        match rx.await {
            Err(_) => Err(Error::custom("Failed to receive batch")),
            Ok(r) => r,
        }
    }

    pub async fn run(self) {
        let mut streams: HashMap<String, RunningEventSource> = HashMap::default();
        let mut outstanding_events: HashMap<String, PendingResults> = HashMap::default();
        let db = Arc::clone(&self.db);
        loop {
            let mut streams_to_process = std::mem::take(&mut streams);
            for (k, mut batcher) in streams_to_process.drain() {
                let entry = outstanding_events.entry(k.clone()).or_default();
                let existing = match db.get_events(&k).await {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("Failed to get events: {:?}", e);
                        vec![]
                    }
                };
                if !existing.is_empty() {
                    tracing::trace!("Restoring {} events", existing.len());
                    entry.events.extend(existing);
                }

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
                std::mem::take(m.deref_mut())
            };
            for (client_id, req) in create_requests.into_iter() {
                let (tx, rx) = tokio::sync::mpsc::channel(100);
                let shutdown = Arc::new(AtomicBool::new(false));
                tokio::spawn(run_event_source(
                    self.ceramic_url.clone(),
                    req.params,
                    shutdown.clone(),
                    tx,
                ));
                let events = db.get_events(&client_id).await.unwrap();
                outstanding_events.insert(
                    client_id.clone(),
                    PendingResults {
                        events,
                        error: None,
                    },
                );
                streams.insert(client_id, RunningEventSource { rx, shutdown });
                req.tx.send(Ok(())).unwrap();
            }
            let get_requests = {
                let mut m = self.stream_get_requests.lock().await;
                std::mem::take(m.deref_mut())
            };
            for (client_id, req) in get_requests.into_iter() {
                if let Some(mut results) = outstanding_events.remove(&client_id) {
                    if results.events.is_empty() {
                        if results.error.is_some() {
                            let err = results.error.take().unwrap();
                            if req.tx.send(Err(err)).is_err() {
                                tracing::debug!("Failed to send error to client");
                            }
                        } else if req.tx.send(Ok(Vec::new())).is_err() {
                            tracing::debug!("Failed to send empty results to client");
                        }
                    } else {
                        let split_idx = std::cmp::min(
                            results.events.len(),
                            req.limit.unwrap_or(usize::max_value()),
                        );
                        let mut rem = results.events.split_off(split_idx);
                        std::mem::swap(&mut results.events, &mut rem);
                        if req.tx.send(Ok(rem)).is_err() {
                            tracing::debug!("Failed to send results to client");
                        }
                    }
                } else if req.tx.send(Err(Error::NotFound(client_id))).is_err() {
                    tracing::debug!("Failed to send not found to client");
                }
            }
            let mut delete_requests = {
                let mut m = self.stream_delete_requests.lock().await;
                std::mem::take(m.deref_mut())
            };
            for client_id in delete_requests.drain() {
                if let Some(s) = streams.get(&client_id) {
                    s.shutdown.store(true, Ordering::Relaxed);
                }
            }
            for (client_id, results) in outstanding_events.iter_mut() {
                let events = std::mem::replace(&mut results.events, vec![]);
                if !events.is_empty() {
                    tracing::trace!("Saving {} events", events.len());
                }
                let mut unpersisted_events = vec![];
                for event in events {
                    if let Err(e) = db.add_event(&client_id, &event).await {
                        tracing::warn!("Failed to persist event: {:?}", e);
                        unpersisted_events.push(event);
                    }
                }
                results.events = unpersisted_events;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

fn event_source_for_params(ceramic_url: &Url, params: &BatchCreationParameters) -> EventSource {
    let ceramic_url = ceramic_url
        .join("/api/v0/feed/aggregation/documents")
        .unwrap();
    tracing::debug!(
        "Creating event source against {} with client {}",
        ceramic_url,
        params.client_id
    );
    EventSource::get(ceramic_url.to_string())
}

async fn run_event_source(
    ceramic_url: Url,
    params: BatchCreationParameters,
    shutdown: Arc<AtomicBool>,
    tx: tokio::sync::mpsc::Sender<Result<Event, Error>>,
) {
    let mut es = event_source_for_params(&ceramic_url, &params);
    while !shutdown.load(Ordering::Relaxed) {
        match es.next().await {
            Some(Ok(res)) => {
                if let SseEvent::Message(msg) = res {
                    tracing::trace!("Message received from ceramic");
                    match serde_json::from_str::<Event>(&msg.data) {
                        Ok(event) => {
                            if tx.send(Ok(event.clone())).await.is_err() {
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
                    tracing::warn!(
                        "Event source for client {} failed: {:?}",
                        params.client_id,
                        e
                    );
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                es = event_source_for_params(&ceramic_url, &params);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BatchCreationParameters, Batcher};
    use crate::errors::Error;
    use crate::persistence::Persistence;
    use ceramic_http_client::{
        ceramic_event::{DidDocument, JwkSigner, Signer, StreamId},
        json_patch, remote, schemars, GetRootSchema, ModelAccountRelation, ModelDefinition,
    };
    use schema::{CeramicMetadata, Event};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    // See https://github.com/ajv-validator/ajv-formats for information on valid formats
    #[derive(Debug, Deserialize, Eq, schemars::JsonSchema, PartialEq, Serialize)]
    #[schemars(rename_all = "camelCase", deny_unknown_fields)]
    struct Ball {
        creator: String,
        radius: i32,
        red: i32,
        green: i32,
        blue: i32,
    }

    impl GetRootSchema for Ball {}

    pub fn ceramic_url() -> url::Url {
        let u =
            std::env::var("CERAMIC_URL").unwrap_or_else(|_| "http://localhost:7007".to_string());
        url::Url::parse(&u).unwrap()
    }

    pub async fn signer() -> JwkSigner {
        let public = std::env::var("DID_DOCUMENT").unwrap_or_else(|_| {
            "did:key:z6MkeqCTPhHPVg3HaAAtsR7vZ6FXkAHPXEbTJs7Y4CQABV9Z".to_string()
        });
        let private =
            std::env::var("DID_PRIVATE_KEY").expect("DID_PRIVATE_KEY not set in environment");
        JwkSigner::new(DidDocument::new(&public), &private)
            .await
            .expect(&format!(
                "Failed to create did for document {public} with key {private}"
            ))
    }

    pub async fn create_model(cli: &remote::CeramicRemoteHttpClient<JwkSigner>) -> StreamId {
        let model = ModelDefinition::new::<Ball>("TestBall", ModelAccountRelation::List).unwrap();
        cli.create_model(&model).await.unwrap()
    }

    struct InMemoryPersistence {
        events: Arc<Mutex<HashMap<String, Vec<Event>>>>,
    }

    impl InMemoryPersistence {
        pub fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl Persistence for InMemoryPersistence {
        async fn add_event(&self, client_id: &str, event: &Event) -> Result<(), Error> {
            let mut events = self.events.lock().await;
            events
                .entry(client_id.to_string())
                .or_insert_with(Vec::new)
                .push(event.clone());
            Ok(())
        }
        async fn get_events(&self, client_id: &str) -> Result<Vec<Event>, Error> {
            let mut events = self.events.lock().await;
            if let Some(evs) = events.get_mut(client_id) {
                Ok(std::mem::replace(evs, Vec::new()))
            } else {
                Ok(Vec::new())
            }
        }
    }

    #[tokio::test]
    async fn should_receive_create_and_update_events() {
        let _guard = util::init_tracing();

        let ceramic_url = ceramic_url();
        let db = Arc::new(InMemoryPersistence::new());
        let batcher = Batcher::new_with_url(db, ceramic_url.clone());
        let client_id = "test";
        batcher
            .create_batcher(BatchCreationParameters {
                client_id: client_id.to_string(),
            })
            .await
            .unwrap();

        tracing::info!("Created batcher with id {}", client_id);

        let ceramic = remote::CeramicRemoteHttpClient::new(signer().await, ceramic_url);
        let model = create_model(&ceramic).await;
        let stream_id = ceramic
            .create_list_instance(
                &model,
                &Ball {
                    creator: ceramic.client().signer().id().id.clone(),
                    radius: 1,
                    red: 2,
                    green: 3,
                    blue: 4,
                },
            )
            .await
            .unwrap();

        //give anchor time to complete
        tokio::time::sleep(Duration::from_secs(1)).await;

        let patch = json_patch::Patch(vec![json_patch::PatchOperation::Replace(
            json_patch::ReplaceOperation {
                path: "/red".to_string(),
                value: serde_json::json!(5),
            },
        )]);
        let post_resp = ceramic.update(&model, &stream_id, patch).await.unwrap();
        assert_eq!(post_resp.stream_id, stream_id);
        let post_resp: Ball = serde_json::from_value(post_resp.state.unwrap().content).unwrap();
        assert_eq!(post_resp.red, 5);

        //give anchor time to complete
        tokio::time::sleep(Duration::from_secs(1)).await;

        let patch = json_patch::Patch(vec![json_patch::PatchOperation::Replace(
            json_patch::ReplaceOperation {
                path: "/blue".to_string(),
                value: serde_json::json!(8),
            },
        )]);
        let post_resp = ceramic.update(&model, &stream_id, patch).await.unwrap();
        assert_eq!(post_resp.stream_id, stream_id);
        let post_resp: Ball = serde_json::from_value(post_resp.state.unwrap().content).unwrap();
        assert_eq!(post_resp.blue, 8);

        //give anchor time to complete
        tokio::time::sleep(Duration::from_secs(1)).await;

        let get_resp: Ball = ceramic.get_as(&stream_id).await.unwrap();
        assert_eq!(get_resp.red, 5);
        assert_eq!(get_resp.blue, 8);
        assert_eq!(get_resp, post_resp);

        let batch = batcher.get_batch(client_id, None).await.unwrap();
        assert!(batch.len() >= 4);
        //model create will have the "parent" model is as the model in metadata
        //we will see the create and the anchor for the model, and then see mids, which have
        //the model as its parent model
        let meta: CeramicMetadata = serde_json::from_value(batch[2].metadata.clone()).unwrap();
        let event_model_id = StreamId::from_str(&meta.model).unwrap();
        assert_eq!(event_model_id, model);
    }
}
