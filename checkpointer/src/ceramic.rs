use ceramic_http_client::api::{self, Pagination};
use ceramic_http_client::ceramic_event::{JwkSigner, StreamId};
use ceramic_http_client::remote::CeramicRemoteHttpClient;
use ceramic_http_client::FilterQuery;
use models::PointMaterialization;

pub struct Ceramic {
    inner: CeramicRemoteHttpClient<JwkSigner>,
}

impl Ceramic {
    pub fn new(inner: CeramicRemoteHttpClient<JwkSigner>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl calculator::Ceramic for Ceramic {
    async fn query(
        &self,
        model_id: &StreamId,
        query: FilterQuery,
    ) -> Result<api::QueryResponse, anyhow::Error> {
        self.inner
            .query(model_id, Some(query), Pagination::default())
            .await
    }

    async fn create(
        &self,
        model_id: &StreamId,
        data: &PointMaterialization,
    ) -> Result<StreamId, anyhow::Error> {
        self.inner.create_list_instance(model_id, data).await
    }

    async fn replace(
        &self,
        model_id: &StreamId,
        stream_id: &StreamId,
        data: &PointMaterialization,
    ) -> Result<StreamId, anyhow::Error> {
        Ok(self
            .inner
            .replace(model_id, stream_id, data)
            .await?
            .stream_id)
    }
}
