use anyhow::Error;
use ceramic_http_client::ceramic_event::StreamId;
use ceramic_http_client::{api, FilterQuery};
use models::PointMaterialization;

#[async_trait::async_trait]
pub trait Ceramic {
    async fn query(
        &self,
        model_id: &StreamId,
        query: FilterQuery,
    ) -> Result<api::QueryResponse, Error>;
    async fn create(
        &self,
        model_id: &StreamId,
        data: &PointMaterialization,
    ) -> Result<StreamId, Error>;
    async fn replace(
        &self,
        model_id: &StreamId,
        stream_id: &StreamId,
        data: &PointMaterialization,
    ) -> Result<StreamId, Error>;
}
