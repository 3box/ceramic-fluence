use ceramic_http_client::{
    schemars::{self, JsonSchema},
    GetRootSchema,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointAttestation {
    pub value: i64,
    pub context: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointAttestations {
    pub holder: String,
    pub issuer: String,
    pub issuer_verification: String,
    pub data: Vec<PointAttestation>,
}

impl GetRootSchema for PointAttestations {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointMaterialization {
    pub issuer: String,
    pub recipient: String,
    pub context: String,
    pub value: i64,
    pub point_claims_id: String,
}

impl GetRootSchema for PointMaterialization {}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub jti: String,
}

pub const AUDIENCE: &str = "points";
