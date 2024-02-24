use base64::prelude::*;
use ceramic_http_client::ceramic_event::StreamId;
use ceramic_http_client::remote::CeramicRemoteHttpClient;
use ceramic_http_client::{
    ceramic_event::{DidDocument, JwkSigner},
    ModelAccountRelation, ModelDefinition,
};
use clap::{Parser, Subcommand};
use hmac::{Hmac, Mac};
use jwt::SignWithKey;
use sha2::digest::Digest;
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
    let client = CeramicRemoteHttpClient::new(signer, url);

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
            let attestations = client
                .create_list_instance(
                    &model,
                    &create_attestations(
                        &did,
                        &pk,
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
                    )?,
                )
                .await?;
            tracing::info!("Attestations created: {}", attestations.to_string());
        }
    }
    Ok(())
}

fn create_attestations(
    did: &DidDocument,
    pk: &str,
    data: Vec<models::PointAttestation>,
) -> Result<models::PointAttestations, anyhow::Error> {
    // hash our data
    let mut hasher = sha2::Sha256::default();
    hasher.update(&serde_json::to_vec(&data)?);
    let jti = BASE64_STANDARD.encode(hasher.finalize());
    let key: Hmac<sha2::Sha256> = hmac::Hmac::new_from_slice(pk.as_bytes())?;
    let claims = models::Claims {
        iss: "ceramic-tester".to_string(),
        sub: did.id.clone(),
        aud: models::AUDIENCE.to_string(),
        jti,
    };
    let token_str = claims.sign_with_key(&key)?;
    Ok(models::PointAttestations {
        issuer: did.id.clone(),
        holder: did.id.clone(),
        issuer_verification: token_str,
        data,
    })
}
