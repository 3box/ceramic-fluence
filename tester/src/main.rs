use ceramic_http_client::json_patch::ReplaceOperation;
use ceramic_http_client::remote::CeramicRemoteHttpClient;
use ceramic_http_client::{
    ceramic_event::{DidDocument, JwkSigner},
    json_patch,
    schemars::{self, JsonSchema},
    GetRootSchema, ModelAccountRelation, ModelDefinition,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(rename_all = "camelCase", deny_unknown_fields)]
struct AttendedEvent1 {
    controller: String,
    jwt: String,
}

impl GetRootSchema for AttendedEvent1 {}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(rename_all = "camelCase", deny_unknown_fields)]
struct AttendedEvent2 {
    controller: String,
    jwt: String,
}

impl GetRootSchema for AttendedEvent2 {}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let _ = util::init_tracing();

    let did = std::env::var("DID_DOCUMENT")
        .unwrap_or_else(|_| "did:key:z6MkeqCTPhHPVg3HaAAtsR7vZ6FXkAHPXEbTJs7Y4CQABV9Z".to_string());
    let signer = JwkSigner::new(
        DidDocument::new(&did),
        &std::env::var("DID_PRIVATE_KEY").unwrap(),
    )
    .await?;

    let url = url::Url::parse("http://localhost:7007").unwrap();
    let client = CeramicRemoteHttpClient::new(signer, url);

    let event1_model =
        ModelDefinition::new::<AttendedEvent1>("AttendedEvent1", ModelAccountRelation::Single)?;
    let event1_model = client.create_model(&event1_model).await?;
    let event2_model =
        ModelDefinition::new::<AttendedEvent1>("AttendedEvent2", ModelAccountRelation::Single)?;
    let _event2_model = client.create_model(&event2_model).await?;

    let event1 = client.create_single_instance(&event1_model).await?;
    let patch = json_patch::Patch(vec![json_patch::PatchOperation::Replace(
        ReplaceOperation {
            path: "controller".to_string(),
            value: serde_json::Value::String(did.clone()),
        },
    )]);
    client.update(&event1_model, &event1, patch).await.unwrap();

    Ok(())
}
