mod ceramic;
mod http;

use http::Http;

use crate::ceramic::Ceramic;
use ceramic_http_client::ceramic_event::{DidDocument, StreamId};
use marine_rs_sdk::{marine, MountedBinaryStringResult};
use schema::Event;
use std::str::FromStr;
use url::Url;
use wasm_rs_async_executor::single_threaded as executor;

#[marine]
// #[link(wasm_import_module = "host")]
#[host_import]
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
    pub attestation_issuer: String,
    pub attestation_model_id: String,
    pub materialization_model_id: String,
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
            log::error!("Error processing events: {}", e);
            SseResponse {
                error: e.to_string(),
                events: 0,
            }
        }
    }
}

const BATCH_PATH: &str = "/api/v1/batch";

const CURL_DEFAULT_ARGUMENTS: &[&str] = &["-H", "Content-Type: application/json", "-i"];

async fn try_process_events(cfg: ExecutionConfig) -> Result<SseResponse, anyhow::Error> {
    let ceramic_endpoint = Url::parse(&cfg.ceramic_endpoint)?;
    let checkpointer_endpoint = Url::parse(&cfg.checkpointer_endpoint)?;
    let client_id = cfg.client_id;
    let attestation_model_id = StreamId::from_str(&cfg.attestation_model_id)?;
    let materialization_model_id = StreamId::from_str(&cfg.materialization_model_id)?;
    let attestation_issuer = DidDocument::new(&cfg.attestation_issuer);

    let did = DidDocument::new(&cfg.public_key);
    let ceramic: Box<dyn calculator::Ceramic + Send + Sync> =
        Box::new(Ceramic::new(did.clone(), &cfg.private_key, ceramic_endpoint).await?);
    let mut calculator = calculator::Calculator::new(
        calculator::CalculatorParameters {
            attestation_issuer,
            attestation_model_id,
            materialization_model_id,
        },
        ceramic,
    );

    let cmd: Vec<_> = CURL_DEFAULT_ARGUMENTS
        .iter()
        .map(|s| s.to_string())
        .chain(vec![
            "-d".to_string(),
            format!(r#"{{"client_id":"{}"}}"#, client_id),
            checkpointer_endpoint.join(BATCH_PATH)?.to_string(),
        ])
        .collect();
    let res = curl(cmd);
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let (resp, _) = Http::parse(&res, &mut headers)?;
    match resp.code {
        Some(200) | Some(409) => {
            log::info!("Consumption started");
        }
        _ => {
            anyhow::bail!("Error({:?}): {:?}", resp.code, resp.reason);
        }
    }
    let now = std::time::Instant::now();
    let mut events_processed = 0u32;
    let cmd: Vec<_> = CURL_DEFAULT_ARGUMENTS
        .iter()
        .map(|s| s.to_string())
        .chain(vec![checkpointer_endpoint
            .join(&format!("{}/{}", BATCH_PATH, client_id))?
            .to_string()])
        .collect();

    loop {
        let res = curl(cmd.clone());
        let events: Vec<Event> = Http::from(res)?;
        log::debug!("Received {} ceramic events", events.len());
        for event in events {
            log::debug!("Processing event: {:?}", event);
            calculator.process_event(event).await?;
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
            attestation_model_id: "attestation".to_string(),
            materialization_model_id: "materialization".to_string(),
        };
        let greeting = iface.process_events(cfg);
        assert!(greeting.error.is_empty());
    }
}
