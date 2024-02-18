use crate::Error;
use schema::Event;
use sqlx::{migrate::MigrateDatabase, Acquire, Connection, Sqlite};

#[derive(sqlx::FromRow)]
pub struct EventRow {
    pub id: String,
    pub client_id: String,
    pub event: sqlx::types::Json<Event>,
}

#[async_trait::async_trait]
pub trait Persistence {
    async fn add_event(&self, client_id: &str, event: &Event) -> Result<(), Error>;
    async fn get_events(&self, client_id: &str) -> Result<Vec<Event>, Error>;
}

#[derive(Clone)]
pub struct SqlitePersistence {
    pool: sqlx::Pool<Sqlite>,
}

impl SqlitePersistence {
    pub async fn new() -> Result<Self, Error> {
        let url =
            std::env::var("DATABASE_URL").map_err(|_| Error::custom("DATABASE_URL was not set"))?;
        Self::new_with_url(&url).await
    }

    pub async fn new_with_url(db: &str) -> Result<Self, Error> {
        tracing::info!("Using database at {}", db);
        let database_exists = Sqlite::database_exists(db).await?;
        if !database_exists {
            tracing::info!("Creating database at {}", db);
            Sqlite::create_database(db).await?;
        }
        let pool = sqlx::sqlite::SqlitePool::connect(db).await?;
        if !database_exists {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS events
(
    id          TEXT PRIMARY KEY NOT NULL,
    client_id   TEXT             NOT NULL,
    event       JSONB            NOT NULL
);",
            )
            .execute(&pool)
            .await?;
        }
        Ok(Self { pool })
    }

    async fn acquire(&self) -> Result<sqlx::pool::PoolConnection<Sqlite>, Error> {
        Ok(self.pool.acquire().await?)
    }
}

#[async_trait::async_trait]
impl Persistence for SqlitePersistence {
    async fn add_event(&self, client_id: &str, event: &Event) -> Result<(), Error> {
        sqlx::query("INSERT INTO events (id, client_id, event) VALUES (?, ?, ?)")
            .bind(event.commit_id.clone())
            .bind(client_id)
            .bind(sqlx::types::Json(event))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_events(&self, client_id: &str) -> Result<Vec<Event>, Error> {
        let mut conn = self.acquire().await?;
        let client_id = client_id.to_string();
        let events: Result<Vec<_>, sqlx::Error> = conn
            .transaction(|txn| {
                Box::pin(async move {
                    let conn = txn.acquire().await?;
                    let events =
                        sqlx::query_as::<_, EventRow>("SELECT * FROM events WHERE client_id = ?")
                            .bind(&client_id)
                            .fetch_all(conn)
                            .await?;
                    let conn = txn.acquire().await?;
                    sqlx::query("DELETE FROM events WHERE client_id = ?")
                        .bind(client_id)
                        .execute(conn)
                        .await?;
                    Ok(events.into_iter().map(|ev| ev.event.0).collect())
                })
            })
            .await;
        Ok(events?)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    async fn setup() -> SqlitePersistence {
        let dir = tmpdir::TmpDir::new("pers").await.unwrap();
        std::env::set_var(
            "DATABASE_URL",
            format!("sqlite://{}/test.db", dir.as_ref().display()),
        );
        SqlitePersistence::new().await.unwrap()
    }

    #[tokio::test]
    async fn can_add_and_retrieve_events() {
        let pool = setup().await;
        let client_id = "test_client";
        let event = Event {
            commit_id: "commit_id".to_string(),
            metadata: serde_json::Value::Null,
            event_type: schema::EventType::Data,
            content: "{}".to_string(),
        };
        pool.add_event(client_id, &event).await.unwrap();
        let events = pool.get_events(client_id).await.unwrap();
        assert_eq!(events[0].commit_id, event.commit_id);
    }
}
