use ceramic_http_client::ceramic_event::StreamId;
use ceramic_http_client::remote::CeramicRemoteHttpClient;
use ceramic_http_client::{
    ceramic_event::{DidDocument, JwkSigner},
    ModelAccountRelation, ModelDefinition,
};
use clap::{Parser, Subcommand};
use std::str::FromStr;

#[derive(Parser)]
#[command(name = "CeramicFluenceTester")]
#[command(version = "1.0")]
#[command(about = "Creates or updates models", long_about = None)]
struct Cli {
    #[clap(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand)]
enum Subcmd {
    CreateModels,
    CreateModelInstances {
        #[clap(short, long)]
        model: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let _guard = util::init_tracing();
    let cmd = Cli::parse();

    let did = std::env::var("DID_DOCUMENT")
        .unwrap_or_else(|_| "did:key:z6MkeqCTPhHPVg3HaAAtsR7vZ6FXkAHPXEbTJs7Y4CQABV9Z".to_string());
    let signer = JwkSigner::new(
        DidDocument::new(&did),
        &std::env::var("DID_PRIVATE_KEY").unwrap(),
    )
    .await?;

    let url = url::Url::parse("http://localhost:7007").unwrap();
    let client = CeramicRemoteHttpClient::new(signer, url);

    match cmd.subcmd {
        Subcmd::CreateModels => {
            let model_definition = ModelDefinition::new::<models::EventAttendance>(
                "EventAttendance",
                ModelAccountRelation::List,
            )?;
            let model = client.create_model(&model_definition).await?;
            client.index_model(&model).await?;
            tracing::info!(
                "Created model: \n   EventAttendance: '{}'",
                model.to_string(),
            );
            let model_definition = ModelDefinition::new::<models::EventAttendancePoints>(
                "EventAttendancePoints",
                ModelAccountRelation::List,
            )?;
            let model = client.create_model(&model_definition).await?;
            client.index_model(&model).await?;
            tracing::info!(
                "Created model: \n   EventAttendancePoints: '{}'",
                model.to_string(),
            );
        }
        Subcmd::CreateModelInstances { model } => {
            let model = StreamId::from_str(&model)?;
            let event = client
                .create_list_instance(
                    &model,
                    &models::EventAttendance {
                        recipient: did.clone(),
                        jwt: "jwt1".to_string(),
                        event: "depin".to_string(),
                    },
                )
                .await?;
            tracing::info!("Depin model instance created: {}", event.to_string());
            let event = client
                .create_list_instance(
                    &model,
                    serde_json::json!({
                        "recipient": did.clone(),
                        "jwt": "jwt1",
                        "event": "proof",
                    }),
                )
                .await?;
            tracing::info!(
                "Proof of data model instance created: {}",
                event.to_string()
            );
        }
    }
    Ok(())
}
