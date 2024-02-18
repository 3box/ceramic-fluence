use crate::{curl, Http, CURL_DEFAULT_ARGUMENTS};
use ceramic_http_client::api::{Pagination, QueryRequest};
use ceramic_http_client::ceramic_event::{Cid, DidDocument, JwkSigner, StreamId};
use ceramic_http_client::{api, CeramicHttpClient, FilterQuery, OperationFilter};
use models::EventAttendancePoints;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::str::FromStr;
use url::Url;

struct ExistingPoints {
    points: EventAttendancePoints,
    stream_id: StreamId,
}

pub struct Ceramic {
    did: DidDocument,
    ceramic_endpoint: Url,
    cli: CeramicHttpClient<JwkSigner>,
    model_id: StreamId,
    points_cache: HashMap<String, ExistingPoints>,
}

impl Ceramic {
    pub async fn new(
        did: DidDocument,
        private_key: &str,
        ceramic_endpoint: Url,
        model_id: StreamId,
    ) -> Result<Self, anyhow::Error> {
        let signer = JwkSigner::new(did.clone(), private_key).await?;
        let cli = CeramicHttpClient::new(signer);
        Ok(Self {
            did,
            ceramic_endpoint,
            cli,
            model_id,
            points_cache: HashMap::default(),
        })
    }

    pub async fn get_points(
        &mut self,
        recipient: &str,
    ) -> Result<EventAttendancePoints, anyhow::Error> {
        if let Some(existing) = self.points_cache.get(recipient) {
            return Ok(existing.points.clone());
        }
        let mut where_filter = HashMap::new();
        where_filter.insert(
            "subject".to_string(),
            OperationFilter::EqualTo(recipient.into()),
        );
        let filter = FilterQuery::Where(where_filter);
        let req = self
            .cli
            .create_query_request(&self.model_id, Some(filter), Pagination::default())
            .await?;
        let res: Vec<(EventAttendancePoints, StreamId)> = self.query(req).await?;
        if let Some((points, stream_id)) = res.into_iter().next() {
            self.points_cache.insert(
                recipient.to_string(),
                ExistingPoints {
                    points: points.clone(),
                    stream_id,
                },
            );
            return Ok(points);
        }
        let points = self.create_points(recipient).await?;
        Ok(points)
    }

    pub async fn update_points(
        &mut self,
        points: EventAttendancePoints,
    ) -> Result<(), anyhow::Error> {
        let stream_id = if let Some(existing) = self.points_cache.get_mut(&points.subject) {
            existing.points = points.clone();
            existing.stream_id.clone()
        } else {
            anyhow::bail!("No existing points for {}", points.subject);
        };
        log::info!("Getting stream {stream_id}");
        let resp = self.get(&stream_id).await?;
        let req = self
            .cli
            .create_replace_request(&self.model_id, &resp, points)
            .await?;
        let res: api::StreamsResponseOrError = self.post(self.cli.commits_endpoint(), req).await?;
        res.resolve("update_points")?;
        Ok(())
    }

    async fn create_points(
        &mut self,
        recipient: &str,
    ) -> Result<EventAttendancePoints, anyhow::Error> {
        let points = models::EventAttendancePoints {
            issuer: self.did.id.to_string(),
            subject: recipient.to_string(),
            scoring: vec![],
        };
        let req = self
            .cli
            .create_list_instance_request(&self.model_id, &points)
            .await?;
        let res: api::StreamsResponseOrError = self.post(self.cli.streams_endpoint(), req).await?;
        let stream_id = res.resolve("create_points")?.stream_id;
        self.points_cache.insert(
            recipient.to_string(),
            ExistingPoints {
                points: points.clone(),
                stream_id,
            },
        );
        Ok(points)
    }

    #[allow(dead_code)]
    async fn get(&self, stream_id: &StreamId) -> Result<api::StreamsResponse, anyhow::Error> {
        let endpoint = format!("{}/{}", self.cli.streams_endpoint(), stream_id);
        let endpoint = self.ceramic_endpoint.join(&endpoint)?;
        log::trace!("Ceramic get {}", endpoint);
        let args: Vec<_> = CURL_DEFAULT_ARGUMENTS
            .iter()
            .map(|s| s.to_string())
            .chain(vec![endpoint.to_string()])
            .collect();
        let resp: api::StreamsResponse = Http::from(curl(args))?;
        Ok(resp)
    }

    async fn post<I: Serialize, O: DeserializeOwned>(
        &self,
        resource: &str,
        req: I,
    ) -> Result<O, anyhow::Error> {
        let endpoint = self.ceramic_endpoint.join(resource)?;
        log::debug!("Ceramic post to {}", endpoint);
        let args: Vec<_> = CURL_DEFAULT_ARGUMENTS
            .iter()
            .map(|s| s.to_string())
            .chain(vec![
                "-d".to_string(),
                serde_json::to_string(&req).unwrap(),
                endpoint.to_string(),
            ])
            .collect();
        let res = curl(args);
        Http::from(res)
    }

    async fn query<T: DeserializeOwned>(
        &self,
        req: QueryRequest,
    ) -> Result<Vec<(T, StreamId)>, anyhow::Error> {
        let resp: api::QueryResponse = self.post(self.cli.collection_endpoint(), req).await?;
        let res: Result<_, anyhow::Error> = resp
            .edges
            .into_iter()
            .map(|edge| {
                log::trace!("Log: {:?}", edge.node.log);
                match (
                    serde_json::from_value(edge.node.content),
                    edge.node
                        .log
                        .into_iter()
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("No log")),
                ) {
                    (Ok(v), Ok(commit)) => {
                        let cid = Cid::from_str(commit.cid.as_ref())?;
                        Ok((v, StreamId::document(cid)))
                    }
                    (Err(e), _) => Err(e.into()),
                    (_, Err(e)) => Err(e),
                }
            })
            .collect();
        res
    }
}
