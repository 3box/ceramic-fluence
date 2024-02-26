use crate::ceramic::Ceramic;
use crate::errors::Error;
use crate::event_source::EventSource;
use ceramic_http_client::ceramic_event::{DidDocument, JwkSigner};
use ceramic_http_client::remote::CeramicRemoteHttpClient;
use schema::{Event, EventType};
use std::str::FromStr;
use url::Url;

#[derive(Clone, Debug)]
pub struct CalculatorParameters {
    pub ceramic_url: Url,
    pub signer: JwkSigner,
}

impl CalculatorParameters {
    pub async fn new() -> Result<Self, Error> {
        let did = std::env::var("DID_DOCUMENT").unwrap_or_else(|_| {
            "did:key:z6Mkk3rtfoKDMMG4zyarNGwCQs44GSQ49pcYKQspHJPXSnVw".to_string()
        });
        let pk =
            std::env::var("DID_PRIVATE_KEY").map_err(|_| Error::custom("Invalid PRIVATE_KEY"))?;
        let url =
            std::env::var("CERAMIC_URL").unwrap_or_else(|_| "http://localhost:70007".to_string());
        let did = DidDocument::new(&did);
        let signer = JwkSigner::new(did.clone(), &pk).await?;
        Ok(Self {
            ceramic_url: Url::from_str(&url)?,
            signer,
        })
    }
}

pub struct Calculator {
    url: Url,
    inner: calculator::Calculator,
}

impl Calculator {
    pub fn new(params: CalculatorParameters) -> Result<Calculator, Error> {
        let url = params.ceramic_url.clone();
        let cli = CeramicRemoteHttpClient::new(params.signer, params.ceramic_url);
        let cli = Box::new(Ceramic::new(cli));
        let calc = calculator::Calculator::new(calculator::CalculatorParameters::new()?, cli);
        Ok(Self { url, inner: calc })
    }

    pub async fn process_event(&mut self, event: Event) -> Result<(), Error> {
        Ok(self.inner.process_event(event).await?)
    }

    pub fn run(self) {
        tokio::spawn(run(self));
    }
}

async fn run(mut calculator: Calculator) {
    let es = EventSource::new("ceramic-calculator", &calculator.url);
    let mut running = es.run();

    tracing::info!("Starting calculator against {}", calculator.url);

    while let Some(event) = running.rx.recv().await {
        match event {
            Ok(event) => {
                if event.event_type == EventType::Data || event.event_type == EventType::Init {
                    if let Err(e) = calculator.process_event(event).await {
                        tracing::error!("Error processing event: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Error receiving event: {}", e);
            }
        }
    }

    tracing::info!("Calculator stopped");
}
