use ceramic_http_client::{
    schemars::{self, JsonSchema},
    GetRootSchema,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventAttendance {
    pub event: String,
    pub recipient: String,
    pub jwt: String,
}

impl GetRootSchema for EventAttendance {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventAttendanceScoring {
    pub points: i32,
    pub algorithm: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventAttendancePoints {
    pub issuer: String,
    pub subject: String,
    pub scoring: Vec<EventAttendanceScoring>,
}

impl GetRootSchema for EventAttendancePoints {}
