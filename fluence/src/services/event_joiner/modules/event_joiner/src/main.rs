use ceramic_http_client::api::Pagination;
use ceramic_http_client::ceramic_event::DidDocument;
use ceramic_http_client::schemars::JsonSchema;
use ceramic_http_client::{
    api,
    ceramic_event::{JwkSigner, StreamId},
    schemars, CeramicHttpClient, FilterQuery, OperationFilter,
};
use httparse::Header;
use log::*;
use marine_rs_sdk::{marine, MountedBinaryStringResult};
use schema::Event;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use url::Url;
use wasm_rs_async_executor::single_threaded as executor;

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(rename_all = "camelCase", deny_unknown_fields)]
struct AggregateEvents {
    user: String,
    unique_events: i32,
    events_attended: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(rename_all = "camelCase", deny_unknown_fields)]
struct EventAttendance {
    controller: String,
    jwt: String,
}

#[marine]
#[link(wasm_import_module = "host")]
extern "C" {
    fn curl(cmd: Vec<String>) -> MountedBinaryStringResult;

}

#[marine]
#[derive(Clone)]
pub struct ExecutionConfig {
    pub client_id: String,
    pub public_key: String,
    pub private_key: String,
    pub ceramic_endpoint: String,
    pub checkpointer_endpoint: String,
    pub depin_stream_id: String,
    pub proof_of_data_stream_id: String,
}

#[marine]
pub struct SseResponse {
    pub error: String,
    pub events: u32,
}

pub fn main() {}

#[marine]
pub fn process_events(cfg: ExecutionConfig) -> SseResponse {
    let _ = env_logger::try_init();

    match executor::block_on(try_process_events(cfg)) {
        Ok(res) => res,
        Err(e) => {
            error!("Error processing events: {}", e);
            SseResponse {
                error: e.to_string(),
                events: 0,
            }
        }
    }
}

async fn try_process_events(cfg: ExecutionConfig) -> Result<SseResponse, anyhow::Error> {
    let ceramic_endpoint = Url::parse(&cfg.ceramic_endpoint)?;
    let checkpointer_endpoint = Url::parse(&cfg.checkpointer_endpoint)?;
    let client_id = cfg.client_id;

    let signer = JwkSigner::new(DidDocument::new(&cfg.public_key), &cfg.private_key).await?;

    let cfg = ProcessConfig {
        ceramic_endpoint,
        cli: CeramicHttpClient::new(signer),
        depin_stream_id: StreamId::from_str(&cfg.depin_stream_id)?,
        proof_of_data_stream_id: StreamId::from_str(&cfg.proof_of_data_stream_id)?,
    };

    let cmd = vec![
        "-H".to_string(),
        "Accept: text/event-stream".to_string(),
        "-d".to_string(),
        format!(r#"{{"client_id":"{}"}}"#, client_id),
        checkpointer_endpoint.join("/batch")?.to_string(),
    ];
    let res = curl(cmd);
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let (resp, _) = parse_http_response(&res, &mut headers)?;
    match resp.code {
        Some(200) | Some(409) => {
            info!("Consumption started");
        }
        _ => {
            anyhow::bail!("Error({:?}): {:?}", resp.code, resp.reason);
        }
    }
    let now = std::time::Instant::now();
    let mut events_processed = 0u32;
    let cmd = vec![checkpointer_endpoint
        .join(&format!("/batch/{}", client_id))?
        .to_string()];

    loop {
        let res = curl(cmd.clone());
        let events: Vec<Event> = from_http_response(res)?;
        info!("Received {} ceramic events", events.len());
        for event in events {
            process_event(&cfg, event).await?;
            events_processed += 1;
        }
        if now.elapsed().as_secs() > 10 {
            break;
        }
    }
    Ok(SseResponse {
        error: String::default(),
        events: events_processed,
    })
}

fn parse_http_response<'a>(
    res: &'a MountedBinaryStringResult,
    headers: &'a mut [Header<'a>],
) -> Result<(httparse::Response<'a, 'a>, &'a [u8]), anyhow::Error> {
    info!("Response: {}", res.stdout);
    let mut resp = httparse::Response::new(headers);
    let bytes = res.stdout.as_bytes();
    let sz = resp.parse(bytes)?.unwrap();
    let rest = &bytes[sz..];
    Ok((resp, rest))
}

fn from_http_response<T: serde::de::DeserializeOwned>(
    res: MountedBinaryStringResult,
) -> Result<T, anyhow::Error> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let (resp, rest) = parse_http_response(&res, &mut headers)?;
    if let Some(200) = resp.code {
        Ok(serde_json::from_slice(rest)?)
    } else {
        Err(anyhow::anyhow!("Error({:?}): {:?}", resp.code, resp.reason))
    }
}

struct ProcessConfig {
    ceramic_endpoint: Url,
    cli: CeramicHttpClient<JwkSigner>,
    depin_stream_id: StreamId,
    proof_of_data_stream_id: StreamId,
}

async fn process_event(cfg: &ProcessConfig, event: Event) -> Result<(), anyhow::Error> {
    info!("Processing event: {:?}", event);
    if let Ok(meta) = serde_json::from_value::<schema::CeramicMetadata>(event.metadata) {
        let controller = meta
            .controllers
            .first()
            .ok_or_else(|| anyhow::anyhow!("No controller"))?;
        let depin_event = query_event(cfg, &cfg.depin_stream_id, controller).await?;
        let data_event = query_event(cfg, &cfg.proof_of_data_stream_id, controller).await?;
        if let (Some(_depin), Some(_data)) = (depin_event, data_event) {
            info!("Attended both depin and data");
        }
    }
    Ok(())
}

async fn query_event(
    cfg: &ProcessConfig,
    model_id: &StreamId,
    controller: &str,
) -> Result<Option<EventAttendance>, anyhow::Error> {
    let mut where_filter = HashMap::new();
    where_filter.insert(
        "controller".to_string(),
        OperationFilter::EqualTo(controller.into()),
    );
    let filter = FilterQuery::Where(where_filter);
    let req = cfg
        .cli
        .create_query_request(model_id, Some(filter), Pagination::default())
        .await?;
    let endpoint = cfg.ceramic_endpoint.join(cfg.cli.collection_endpoint())?;
    let args = vec![
        "-H".to_string(),
        "Content-Type: application/json".to_string(),
        "-d".to_string(),
        serde_json::to_string(&req).unwrap(),
        endpoint.to_string(),
    ];
    let res = curl(args);
    let resp: api::QueryResponse = from_http_response(res)?;
    if let Some(edge) = resp.edges.into_iter().next() {
        let res: EventAttendance = serde_json::from_value(edge.node.content).unwrap();
        return Ok(Some(res));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use marine_rs_sdk_test::marine_test;

    #[marine_test(config_path = "../../../../../../../.fluence/tmp/Config.toml")]
    fn test_get_events(iface: marine_test_env::event_joiner::ModuleInterface) {
        let cfg = marine_test_env::event_joiner::ExecutionConfig {
            client_id: "client".to_string(),
            public_key: std::env::var("DID_DOCUMENT").unwrap_or_else(|_| {
                "did:key:z6MkeqCTPhHPVg3HaAAtsR7vZ6FXkAHPXEbTJs7Y4CQABV9Z".to_string()
            }),
            private_key: std::env::var("DID_PRIVATE_KEY").unwrap(),
            checkpointer_endpoint: std::env::var("CHECKPOINTER_URL")
                .unwrap_or_else(|_| "http://localhost:8080".to_string()),
            ceramic_endpoint: std::env::var("CERAMIC_URL")
                .unwrap_or_else(|_| "http://localhost:8080".to_string()),
            depin_stream_id: "depin".to_string(),
            proof_of_data_stream_id: "proof".to_string(),
        };
        let greeting = iface.process_events(cfg);
        assert!(greeting.error.is_empty());
    }
}
