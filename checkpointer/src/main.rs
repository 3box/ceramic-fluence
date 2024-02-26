use actix_web::middleware::{ErrorHandlerResponse, ErrorHandlers, Logger};
use actix_web::{
    delete, dev::ServiceResponse, get, http::StatusCode, post, web, App, HttpResponse, HttpServer,
    Responder, Result,
};
use serde::Deserialize;
use tracing_actix_web::TracingLogger;

mod batcher;
mod calculator;
mod ceramic;
mod errors;
mod event_source;
mod persistence;

use crate::persistence::SqlitePersistence;
use batcher::{BatchCreationParameters, Batcher};
use calculator::CalculatorParameters;
use errors::Error;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn trace_error<B>(res: ServiceResponse<B>) -> Result<ErrorHandlerResponse<B>> {
    if let Some(ref e) = res.response().error() {
        tracing::error!("{:?}", e);
    }

    Ok(ErrorHandlerResponse::Response(res.map_into_left_body()))
}

#[post("/batch")]
pub async fn create_batcher(
    config: web::Data<Config>,
    data: web::Json<BatchCreationParameters>,
) -> Result<impl Responder, Error> {
    let res = match config.batcher.create_batcher(data.into_inner()).await {
        Ok(_) => HttpResponse::Ok().finish(),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    };
    Ok(res)
}

#[derive(Debug, Deserialize)]
pub struct BatchQueryParameters {
    pub limit: Option<usize>,
}

#[get("/batch/{client_id}")]
pub async fn get_batch(
    config: web::Data<Config>,
    client_id: web::Path<String>,
    query: web::Query<BatchQueryParameters>,
) -> Result<impl Responder, Error> {
    let res = config.batcher.get_batch(&client_id, query.limit).await;
    let res = match res {
        Ok(res) => {
            tracing::info!("Events:{:?}", res);
            HttpResponse::Ok().json(res)
        }
        Err(Error::NotFound(id)) => {
            HttpResponse::NotFound().body(format!("Batcher {} not found", id))
        }
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    };
    Ok(res)
}

#[delete("/batch/{client_id}")]
pub async fn delete_batcher(
    _config: web::Data<Config>,
    _client_id: web::Path<String>,
) -> Result<impl Responder, Error> {
    Ok(HttpResponse::Ok().finish())
}

#[post("/calculate")]
pub async fn calculate(config: web::Data<Config>) -> Result<impl Responder, Error> {
    if config
        .calculate_active
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        Ok(HttpResponse::Conflict().body("Calculation already in progress"))
    } else {
        tracing::info!("Starting calculation");
        config
            .calculate_active
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let calculator = calculator::Calculator::new(config.calculator_params.clone())?;
        calculator.run();
        Ok(HttpResponse::Ok().finish())
    }
}

#[get("/healthcheck")]
pub async fn healthcheck() -> impl Responder {
    HttpResponse::Ok().finish()
}

#[derive(Clone)]
pub struct Config {
    batcher: Batcher,
    calculator_params: CalculatorParameters,
    calculate_active: Arc<AtomicBool>,
}

#[actix_web::main]
async fn main() -> Result<(), Error> {
    let _guard = util::init_tracing();

    let config = Config {
        batcher: Batcher::new(Arc::new(SqlitePersistence::new().await?))?,
        calculator_params: CalculatorParameters::new().await?,
        calculate_active: Arc::new(AtomicBool::new(false)),
    };

    HttpServer::new(move || {
        let svc = web::scope("/api/v1")
            .service(create_batcher)
            .service(get_batch)
            .service(delete_batcher)
            .service(calculate)
            .service(healthcheck);
        App::new()
            .wrap(TracingLogger::default())
            .wrap(ErrorHandlers::new().handler(StatusCode::INTERNAL_SERVER_ERROR, trace_error))
            .wrap(actix_web::middleware::Compress::default())
            .wrap(Logger::new(
                "%a %t \"%r\" %s %b \"%{Referer}i\" \"%{User-Agent}i\" %T",
            ))
            .app_data(web::Data::new(config.clone()))
            .service(svc)
    })
    .bind(("0.0.0.0", 8080))
    .map_err(errors::Error::Bind)?
    .run()
    .await?;
    Ok(())
}
