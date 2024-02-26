use crate::materialization_cache::MaterializationCache;
use crate::Ceramic;
use base64::prelude::*;
use ceramic_http_client::ceramic_event::{ssi, DidDocument, Jwk, StreamId};
use itertools::Itertools;
use models::PointAttestations;
use schema::Event;
use std::collections::HashSet;
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct CalculatorParameters {
    pub attestation_model_id: StreamId,
    pub materialization_model_id: StreamId,
}

impl CalculatorParameters {
    pub fn new() -> Result<Self, anyhow::Error> {
        let attestation_model_id = std::env::var("ATTESTATION_MODEL_ID").unwrap_or_else(|_| {
            "kjzl6hvfrbw6c947qf7ucq427v0eocaq4no93zdtccy70o5gclmcvyxjbqrx8mo".to_string()
        });
        let materialization_model_id =
            std::env::var("MATERIALIZATION_MODEL_ID").unwrap_or_else(|_| {
                "kjzl6hvfrbw6c88slfzg2mw6jvin2hgv2v24tbl9u0xc97f4pr4755xjr2l6sck".to_string()
            });
        Ok(Self {
            attestation_model_id: StreamId::from_str(&attestation_model_id)?,
            materialization_model_id: StreamId::from_str(&materialization_model_id)?,
        })
    }
}

pub struct Calculator {
    params: CalculatorParameters,
    cache: MaterializationCache,
}

impl Calculator {
    pub fn new(params: CalculatorParameters, cli: Box<dyn Ceramic + Send + Sync>) -> Calculator {
        let cache = MaterializationCache::new(&params.materialization_model_id, cli);
        Self { params, cache }
    }

    pub async fn process_event(&mut self, event: Event) -> Result<(), anyhow::Error> {
        let meta: schema::CeramicMetadata = serde_json::from_value(event.metadata)?;
        let model = StreamId::from_str(&meta.model)?;
        if model != self.params.attestation_model_id {
            tracing::debug!("Skipping event for model {}", model);
            return Ok(());
        }
        let holder = meta
            .controllers
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No controllers for event"))?;
        let attestation_stream_id = StreamId::from_str(&event.commit_id)?;
        match serde_json::from_str::<PointAttestations>(&event.content) {
            Ok(attestation) => {
                if let Err(e) = validate_attestation(&attestation).await {
                    tracing::warn!("Error validating attestation: {}", e);
                    return Ok(());
                }
                unique_events(
                    &mut self.cache,
                    &holder,
                    &attestation,
                    &attestation_stream_id,
                )
                .await?;
                all_events(
                    &mut self.cache,
                    &holder,
                    &attestation,
                    &attestation_stream_id,
                )
                .await?;
                first_all_events(
                    &mut self.cache,
                    &holder,
                    &attestation,
                    &attestation_stream_id,
                )
                .await?;
            }
            Err(e) => {
                tracing::warn!("Error parsing attestation: {}\n{}", e, event.content);
            }
        }
        Ok(())
    }
}

async fn validate_attestation(attestation: &PointAttestations) -> Result<(), anyhow::Error> {
    let did = DidDocument::new(&attestation.issuer);
    let bytes = BASE64_STANDARD.decode(attestation.issuer_verification.as_bytes())?;
    let sig = String::from_utf8_lossy(&bytes);
    let (_, _, s) = ssi::jws::split_jws(&sig)?;
    let jwk = Jwk::new(&did).await?;
    ssi::jws::verify_bytes(ssi::jwk::Algorithm::EdDSA, &bytes, &jwk, s.as_bytes())?;

    Ok(())
}

const UNIQUE_EVENTS_CONTEXT: &str = "unique-events";
async fn unique_events(
    cache: &mut MaterializationCache,
    holder: &str,
    attestation: &PointAttestations,
    attestation_stream_id: &StreamId,
) -> Result<(), anyhow::Error> {
    let keys: HashSet<_> = attestation
        .data
        .iter()
        .group_by(|d| &d.context)
        .into_iter()
        .map(|t| t.0)
        .collect();
    match cache.get_points(holder, UNIQUE_EVENTS_CONTEXT).await? {
        Some(mut existing) => {
            existing.points.value = keys.len() as i64;
            tracing::info!(
                "Updating points for {}: {:?}",
                UNIQUE_EVENTS_CONTEXT,
                existing.points
            );
            cache.update_points(existing).await?;
        }
        None => {
            tracing::info!(
                "Creating points for holder {} for {}",
                holder,
                UNIQUE_EVENTS_CONTEXT
            );
            cache
                .create_points(
                    holder,
                    UNIQUE_EVENTS_CONTEXT,
                    attestation_stream_id,
                    keys.len() as i64,
                )
                .await?;
        }
    }
    Ok(())
}

const ALL_EVENTS_CONTEXT: &str = "all-events";
async fn all_events(
    cache: &mut MaterializationCache,
    holder: &str,
    attestation: &PointAttestations,
    attestation_stream_id: &StreamId,
) -> Result<(), anyhow::Error> {
    let keys: Vec<_> = attestation
        .data
        .iter()
        .group_by(|d| &d.context)
        .into_iter()
        .map(|t| t.0)
        .collect();
    match cache
        .get_points(holder, crate::calculator::ALL_EVENTS_CONTEXT)
        .await?
    {
        Some(mut existing) => {
            existing.points.value = keys.len() as i64;
            tracing::info!(
                "Updating points for {}: {:?}",
                ALL_EVENTS_CONTEXT,
                existing.points
            );
            cache.update_points(existing).await?;
        }
        None => {
            tracing::info!(
                "Creating points for recipient {} for {}",
                holder,
                ALL_EVENTS_CONTEXT
            );
            cache
                .create_points(
                    holder,
                    ALL_EVENTS_CONTEXT,
                    attestation_stream_id,
                    keys.len() as i64,
                )
                .await?;
        }
    }
    Ok(())
}

const FIRST_ALL_EVENTS_CONTEXT: &str = "first-all-events";
const TOTAL_EVENTS: usize = 9;
async fn first_all_events(
    cache: &mut MaterializationCache,
    holder: &str,
    attestation: &PointAttestations,
    attestation_stream_id: &StreamId,
) -> Result<(), anyhow::Error> {
    let events_by_last_time: Vec<_> = attestation
        .data
        .iter()
        .group_by(|d| &d.context)
        .into_iter()
        .flat_map(|(_, group)| {
            group
                .into_iter()
                .sorted_by(|a, b| a.timestamp.cmp(&b.timestamp))
                .next_back()
        })
        .sorted_by(|a, b| a.timestamp.cmp(&b.timestamp))
        .rev()
        .collect();
    if events_by_last_time.len() >= TOTAL_EVENTS
        && cache
            .get_points(holder, FIRST_ALL_EVENTS_CONTEXT)
            .await?
            .is_none()
    {
        tracing::info!(
            "Creating points for recipient {} for {}",
            holder,
            FIRST_ALL_EVENTS_CONTEXT
        );
        cache
            .create_points(
                holder,
                FIRST_ALL_EVENTS_CONTEXT,
                attestation_stream_id,
                events_by_last_time.first().unwrap().timestamp.timestamp(),
            )
            .await?;
    }
    Ok(())
}
