use crate::{curl, Http, CURL_DEFAULT_ARGUMENTS};
use anyhow::Error;
use ceramic_http_client::api::Pagination;
use ceramic_http_client::ceramic_event::{DidDocument, JwkSigner, StreamId};
use ceramic_http_client::{api, CeramicHttpClient, FilterQuery};
use models::PointMaterialization;
use serde::de::DeserializeOwned;
use serde::Serialize;
use url::Url;

pub struct Ceramic {
    ceramic_endpoint: Url,
    cli: CeramicHttpClient<JwkSigner>,
}

impl Ceramic {
    pub async fn new(
        did: DidDocument,
        private_key: &str,
        ceramic_endpoint: Url,
    ) -> Result<Self, anyhow::Error> {
        let signer = JwkSigner::new(did.clone(), private_key).await?;
        let cli = CeramicHttpClient::new(signer);
        Ok(Self {
            ceramic_endpoint,
            cli,
        })
    }

    async fn get<T: DeserializeOwned>(&self, resource: &str) -> Result<T, Error> {
        let endpoint = self.ceramic_endpoint.join(resource)?;
        log::debug!("Ceramic get to {}", endpoint);
        let args: Vec<_> = CURL_DEFAULT_ARGUMENTS
            .iter()
            .map(|s| s.to_string())
            .chain(vec![endpoint.to_string()])
            .collect();
        let res = curl(args);
        Http::from(res)
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
}

#[async_trait::async_trait]
impl calculator::Ceramic for Ceramic {
    async fn query(
        &self,
        model_id: &StreamId,
        query: FilterQuery,
    ) -> Result<api::QueryResponse, Error> {
        let req = self
            .cli
            .create_query_request(model_id, Some(query), Pagination::default())
            .await?;
        let resp: api::QueryResponse = self.post(self.cli.collection_endpoint(), req).await?;
        Ok(resp)
    }

    async fn replace(
        &self,
        model_id: &StreamId,
        stream_id: &StreamId,
        data: &PointMaterialization,
    ) -> Result<StreamId, Error> {
        let resource = format!("{}/{}", self.cli.streams_endpoint(), stream_id);
        let resp = self.get(&resource).await?;
        let req = self
            .cli
            .create_replace_request(model_id, &resp, data)
            .await?;
        let res: api::StreamsResponseOrError = self.post(self.cli.commits_endpoint(), req).await?;
        let stream_id = res.resolve("update_points")?.stream_id;
        Ok(stream_id)
    }

    async fn create(
        &self,
        model_id: &StreamId,
        data: &PointMaterialization,
    ) -> Result<StreamId, Error> {
        let req = self
            .cli
            .create_list_instance_request(model_id, data)
            .await?;
        let res: api::StreamsResponseOrError = self.post(self.cli.streams_endpoint(), req).await?;
        let stream_id = res.resolve("create_points")?.stream_id;
        Ok(stream_id)
    }
}
