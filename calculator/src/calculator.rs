use crate::materialization_cache::MaterializationCache;
use crate::Ceramic;
use base64::prelude::*;
use ceramic_http_client::ceramic_event::StreamId;
use hmac::{Hmac, Mac};
use itertools::Itertools;
use jwt::VerifyWithKey;
use models::PointAttestations;
use schema::Event;
use sha2::Digest;
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
        let attestation_stream_id = StreamId::from_str(&event.commit_id)?;
        match serde_json::from_str::<PointAttestations>(&event.content) {
            Ok(attestation) => {
                if let Err(e) = validate_attestation(&attestation) {
                    tracing::warn!("Error validating attestation: {}", e);
                    return Ok(());
                }
                unique_events(&mut self.cache, &attestation, &attestation_stream_id).await?;
                all_events(&mut self.cache, &attestation, &attestation_stream_id).await?;
                first_all_events(&mut self.cache, &attestation, &attestation_stream_id).await?;
            }
            Err(e) => {
                tracing::warn!("Error parsing attestation: {}", e);
            }
        }
        Ok(())
    }
}

fn validate_attestation(attestation: &PointAttestations) -> Result<(), anyhow::Error> {
    let mut hasher = sha2::Sha256::default();
    hasher.update(&serde_json::to_vec(&attestation.data)?);
    let expected_jti = BASE64_STANDARD.encode(hasher.finalize());
    let key: Hmac<sha2::Sha256> = Hmac::new_from_slice(attestation.issuer.as_bytes())?;
    let token: jwt::Token<jwt::Header, models::Claims, _> = attestation
        .issuer_verification
        .as_str()
        .verify_with_key(&key)?;
    if token.claims().jti != expected_jti {
        anyhow::bail!("Invalid JTI");
    }
    Ok(())
}

const UNIQUE_EVENTS_CONTEXT: &str = "unique-events";
async fn unique_events(
    cache: &mut MaterializationCache,
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
    match cache
        .get_points(&attestation.holder, UNIQUE_EVENTS_CONTEXT)
        .await?
    {
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
                attestation.holder,
                UNIQUE_EVENTS_CONTEXT
            );
            cache
                .create_points(
                    &attestation.holder,
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
        .get_points(&attestation.holder, crate::calculator::ALL_EVENTS_CONTEXT)
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
                attestation.holder,
                ALL_EVENTS_CONTEXT
            );
            cache
                .create_points(
                    &attestation.holder,
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
            .get_points(&attestation.holder, FIRST_ALL_EVENTS_CONTEXT)
            .await?
            .is_none()
    {
        tracing::info!(
            "Creating points for recipient {} for {}",
            attestation.holder,
            FIRST_ALL_EVENTS_CONTEXT
        );
        cache
            .create_points(
                &attestation.holder,
                FIRST_ALL_EVENTS_CONTEXT,
                attestation_stream_id,
                events_by_last_time.first().unwrap().timestamp.timestamp(),
            )
            .await?;
    }
    Ok(())
}
