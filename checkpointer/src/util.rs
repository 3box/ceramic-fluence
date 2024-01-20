use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

pub fn init_tracing() -> tracing_appender::non_blocking::WorkerGuard {
    let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());
    let fmt = std::env::var("RUST_LOG_FORMAT").unwrap_or_else(|_| "default".to_string());
    match fmt.to_lowercase().as_str() {
        "pretty" => {
            tracing_subscriber::fmt()
                .pretty()
                .with_env_filter(EnvFilter::from_default_env())
                .with_writer(non_blocking)
                .finish()
                .init();
        }
        "json" => {
            tracing_subscriber::fmt()
                .json() // should use JSON or opentelemetry in prod
                .with_env_filter(EnvFilter::from_default_env())
                .with_writer(non_blocking)
                .finish()
                .init();
        }
        _f => {
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::from_default_env())
                .with_writer(non_blocking)
                .finish()
                .init();
        }
    }
    guard
}
