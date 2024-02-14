mod ceramic;
mod http;

use http::Http;

use crate::ceramic::Ceramic;
use ceramic_http_client::ceramic_event::{DidDocument, StreamId};
use marine_rs_sdk::{marine, MountedBinaryStringResult};
use models::EventAttendanceScoring;
use schema::Event;
use std::str::FromStr;
use url::Url;
use wasm_rs_async_executor::single_threaded as executor;

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
    pub attendance_model_id: String,
    pub points_model_id: String,
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

const BATCH_PATH: &'static str = "/api/v1/batch";

const CURL_DEFAULT_ARGUMENTS: &'static [&'static str] =
    &["-H", "Content-Type: application/json", "-i"];

async fn try_process_events(cfg: ExecutionConfig) -> Result<SseResponse, anyhow::Error> {
    let ceramic_endpoint = Url::parse(&cfg.ceramic_endpoint)?;
    let checkpointer_endpoint = Url::parse(&cfg.checkpointer_endpoint)?;
    let client_id = cfg.client_id;
    let attendance_model_id = StreamId::from_str(&cfg.attendance_model_id)?;

    let did = DidDocument::new(&cfg.public_key);
    let mut ceramic = Ceramic::new(
        did.clone(),
        &cfg.private_key,
        ceramic_endpoint,
        StreamId::from_str(&cfg.points_model_id)?,
    )
    .await?;

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
            process_event(&mut ceramic, &attendance_model_id, event).await?;
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

async fn process_event(
    ceramic: &mut Ceramic,
    attendance_model_id: &StreamId,
    event: Event,
) -> Result<(), anyhow::Error> {
    log::debug!("Processing event: {:?}", event);
    let meta: schema::CeramicMetadata = serde_json::from_value(event.metadata)?;
    let model = StreamId::from_str(&meta.model)?;
    if model != *attendance_model_id {
        log::debug!("Skipping event for model {}", model);
        return Ok(());
    }
    if let Ok(attendance) = serde_json::from_str::<models::EventAttendance>(&event.content) {
        let mut points = ceramic.get_points(&attendance.recipient).await?;
        let alg = format!("ceramicfluence-{}", attendance.event);
        if let Some(score) = points.scoring.iter_mut().find(|s| s.algorithm == alg) {
            log::debug!(
                "Additional attendance of {} for {}",
                attendance.recipient,
                attendance.event
            );
            score.points += 1;
        } else {
            log::info!(
                "First attendance of {} for {}",
                attendance.recipient,
                attendance.event
            );
            points.scoring.push(EventAttendanceScoring {
                points: 1,
                algorithm: alg,
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
        log::info!("Updating points for {}", attendance.recipient);
        ceramic.update_points(points).await?;
    }
    Ok(())
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
            stream_id: "stream".to_string(),
        };
        let greeting = iface.process_events(cfg);
        assert!(greeting.error.is_empty());
    }
}
