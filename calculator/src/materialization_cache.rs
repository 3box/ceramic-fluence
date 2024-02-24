use crate::ceramic::Ceramic;
use anyhow::Error;
use ceramic_http_client::api::QueryNode;
use ceramic_http_client::ceramic_event::{Cid, StreamId};
use ceramic_http_client::{FilterQuery, OperationFilter};
use models::PointMaterialization;
use std::collections::{hash_map::Entry, HashMap};
use std::str::FromStr;

#[derive(Clone)]
pub struct ExistingPoints {
    pub points: PointMaterialization,
    pub stream_id: StreamId,
}

pub struct MaterializationCache {
    model_id: StreamId,
    cli: Box<dyn Ceramic + Send + Sync>,
    cache: HashMap<(String, String), ExistingPoints>,
}

impl MaterializationCache {
    pub fn new(model_id: &StreamId, cli: Box<dyn Ceramic + Send + Sync>) -> Self {
        Self {
            model_id: model_id.clone(),
            cli,
            cache: HashMap::default(),
        }
    }

    pub async fn get_points(
        &mut self,
        subject: &str,
        context: &str,
    ) -> Result<Option<ExistingPoints>, Error> {
        match self.cache.entry((subject.to_string(), context.to_string())) {
            Entry::Occupied(entry) => Ok(Some(entry.get().clone())),
            Entry::Vacant(entry) => {
                let mut where_filter = HashMap::new();
                where_filter.insert(
                    "subject".to_string(),
                    OperationFilter::EqualTo(subject.into()),
                );
                let filter = FilterQuery::Where(where_filter);
                let resp = self.cli.query(&self.model_id, filter).await?;
                let res: Result<Vec<_>, Error> =
                    resp.edges.into_iter().map(|v| convert(v.node)).collect();
                if let Some((points, stream_id)) = res?.into_iter().next() {
                    let existing = ExistingPoints { points, stream_id };
                    entry.insert(existing.clone());
                    return Ok(Some(existing));
                }
                Ok(None)
            }
        }
    }

    pub async fn create_points(
        &mut self,
        subject: &str,
        context: &str,
        point_attestation_id: &StreamId,
        value: i64,
    ) -> Result<ExistingPoints, Error> {
        let points = PointMaterialization {
            issuer: "ceramic-fluence".to_string(),
            recipient: subject.to_string(),
            context: context.to_string(),
            value,
            point_claims_id: point_attestation_id.to_string(),
        };
        let stream_id = self.cli.create(&self.model_id, &points).await?;
        let existing = ExistingPoints { points, stream_id };
        self.cache.insert(
            (
                existing.points.recipient.clone(),
                existing.points.context.clone(),
            ),
            existing.clone(),
        );
        Ok(existing)
    }

    pub async fn update_points(
        &mut self,
        existing: ExistingPoints,
    ) -> Result<ExistingPoints, Error> {
        let updated_id = self
            .cli
            .replace(&self.model_id, &existing.stream_id, &existing.points)
            .await?;
        let existing = ExistingPoints {
            points: existing.points,
            stream_id: updated_id,
        };
        self.cache.insert(
            (
                existing.points.recipient.clone(),
                existing.points.context.clone(),
            ),
            existing.clone(),
        );
        Ok(existing)
    }
}

fn convert(node: QueryNode) -> Result<(PointMaterialization, StreamId), Error> {
    let mat = serde_json::from_value(node.content)?;
    let commit = node
        .log
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No log"))?;
    let cid = Cid::from_str(commit.cid.as_ref())?;
    Ok((mat, StreamId::document(cid)))
}
