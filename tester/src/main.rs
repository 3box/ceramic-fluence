use ceramic_http_client::ceramic_event::{Signer, StreamId};
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
    CreateAttestations {
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
    let did = DidDocument::new(&did);
    let pk = std::env::var("DID_PRIVATE_KEY").unwrap();
    let signer = JwkSigner::new(did.clone(), &pk).await?;

    let url = std::env::var("CERAMIC_URL").unwrap_or_else(|_| "http://localhost:7007".to_string());
    let url = url::Url::parse(&url)?;
    tracing::info!("Connecting to Ceramic node at: {}", url);
    let client = CeramicRemoteHttpClient::new(signer.clone(), url);

    match cmd.subcmd {
        Subcmd::CreateModels => {
            let model_definition = ModelDefinition::new::<models::PointAttestations>(
                "PointAttestations",
                ModelAccountRelation::List,
            )?;
            let model = client.create_model(&model_definition).await?;
            client.index_model(&model).await?;
            tracing::info!(
                "Created model: \n   PointAttestations: '{}'",
                model.to_string(),
            );
            let model_definition = ModelDefinition::new::<models::PointMaterialization>(
                "PointMaterialization",
                ModelAccountRelation::List,
            )?;
            let model = client.create_model(&model_definition).await?;
            client.index_model(&model).await?;
            tracing::info!(
                "Created model: \n   PointMaterialization: '{}'",
                model.to_string(),
            );
        }
        Subcmd::CreateAttestations { model } => {
            let model = StreamId::from_str(&model)?;
            let attestations = create_attestations(
                &signer,
                vec![
                    models::PointAttestation {
                        value: 1,
                        context: "proof-of-data".to_string(),
                        timestamp: chrono::Utc::now(),
                        ref_id: None,
                    },
                    models::PointAttestation {
                        value: 1,
                        context: "proof-of-data".to_string(),
                        timestamp: chrono::Utc::now() + chrono::Duration::hours(1),
                        ref_id: None,
                    },
                    models::PointAttestation {
                        value: 1,
                        context: "depin".to_string(),
                        timestamp: chrono::Utc::now(),
                        ref_id: None,
                    },
                ],
            )
            .await?;
            let attestations_id = client.create_list_instance(&model, &attestations).await?;
            tracing::info!("Attestations created: {}", attestations_id.to_string());
        }
    }
    Ok(())
}

async fn create_attestations(
    signer: &JwkSigner,
    data: Vec<models::PointAttestation>,
) -> Result<models::PointAttestations, anyhow::Error> {
    let verification = signer.sign(&serde_json::to_vec(&data)?).await?;
    Ok(models::PointAttestations {
        issuer: signer.id().id.clone(),
        holder: signer.id().id.clone(),
        issuer_verification: verification.to_string(),
        data,
    })
}
