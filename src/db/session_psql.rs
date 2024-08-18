use std::collections::HashMap;
use std::future::Future;
use std::str::FromStr;

use actix_session::storage::{LoadError, SaveError, SessionKey, SessionStore, UpdateError};
use actix_web::cookie::time::Duration;
use anyhow::Error;
use chrono::Local;
use sqlx::{Executor, PgPool, QueryBuilder};
use sqlx::postgres::PgPoolOptions;
use sqlx::types::{Json, Uuid};

#[derive(Debug, Clone)]
pub struct PostgreSQLSessionStore {
    pool: PgPool,
}

impl PostgreSQLSessionStore {
    pub async fn new(connection_uri: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new().connect(connection_uri).await?;

        let mut tx = pool.begin().await?;
        let table = include_str!("sql/session_store.sql");
        tx.execute(table).await?;
        tx.commit().await?;

        Ok(Self { pool })
    }

    pub async fn from_env(key: &str) -> Result<Self, sqlx::Error> {
        let key = std::env::var(key).expect("Missing Database URI");
        Self::new(&key).await
    }
}

impl SessionStore for PostgreSQLSessionStore {
    fn load(
        &self,
        session_key: &SessionKey,
    ) -> impl Future<Output = Result<Option<HashMap<String, String>>, LoadError>> {
        let output = async {
            let mut builder = QueryBuilder::new("SELECT state FROM session_store WHERE id = ");

            let session_key = session_key.as_ref();
            let session_key = Uuid::from_str(&session_key).expect("Failed to parse uuid key");
            builder.push_bind(session_key);

            let output = builder
                .build_query_scalar::<Json<HashMap<String, String>>>()
                .fetch_optional(&self.pool)
                .await;

            match output {
                Ok(Some(data)) => Ok(Some(data.0)),
                Ok(None) => Ok(None),
                Err(err) => Err(LoadError::Other(Error::new(err))),
            }
        };

        output
    }

    fn save(
        &self,
        session_state: HashMap<String, String>,
        ttl: &Duration,
    ) -> impl Future<Output = Result<SessionKey, SaveError>> {
        let output = async {
            let mut builder = QueryBuilder::new("INSERT INTO session_store(state, ttl) VALUES (");
            builder.push_bind(Json(session_state));

            builder.push(", ");
            builder.push_bind(ttl.as_seconds_f64());

            builder.push(") RETURNING id");

            let output = builder
                .build_query_scalar::<Uuid>()
                .fetch_one(&self.pool)
                .await;

            match output {
                Ok(key) => {
                    let key = key.to_string();
                    Ok(SessionKey::try_from(key).unwrap())
                }
                Err(err) => Err(SaveError::Other(Error::new(err))),
            }
        };

        output
    }

    fn update(
        &self,
        session_key: SessionKey,
        session_state: HashMap<String, String>,
        ttl: &Duration,
    ) -> impl Future<Output = Result<SessionKey, UpdateError>> {
        let session_key_uuid = Uuid::from_str(session_key.as_ref()).expect("Failed to parse key");
        let session_key_uuid = session_key_uuid.clone();

        let output = async move {
            let mut builder = QueryBuilder::new("UPDATE session_store SET state = ");
            builder.push_bind(Json(session_state));

            builder.push(", ttl = ");
            builder.push_bind(ttl.as_seconds_f64());

            builder.push(", updated_at = ");
            builder.push_bind(Local::now());

            builder.push(" WHERE id = ");
            builder.push_bind(session_key_uuid);

            let output = builder.build().execute(&self.pool).await;

            match output {
                Ok(_) => Ok(session_key),
                Err(err) => Err(UpdateError::Other(Error::new(err))),
            }
        };

        output
    }

    fn update_ttl(
        &self,
        session_key: &SessionKey,
        ttl: &Duration,
    ) -> impl Future<Output = Result<(), Error>> {
        let output = async {
            let mut builder = QueryBuilder::new("UPDATE session_store SET ttl = ");
            builder.push_bind(ttl.as_seconds_f64());

            builder.push(", updated_at = ");
            builder.push_bind(Local::now());

            builder.push(" WHERE id = ");
            let session_key = Uuid::from_str(session_key.as_ref()).expect("Failed to parse key");
            builder.push_bind(&session_key);

            let output = builder.build().execute(&self.pool).await;

            match output {
                Ok(_) => Ok(()),
                Err(err) => Err(err.into()),
            }
        };

        output
    }

    fn delete(&self, session_key: &SessionKey) -> impl Future<Output = Result<(), Error>> {
        let session_key = Uuid::from_str(session_key.as_ref()).expect("Failed to parse key");
        let session_key = session_key.clone();
        let output = async move {
            let mut builder = QueryBuilder::new("DELETE FROM session_store WHERE id = ");
            builder.push_bind(session_key);

            let output = builder.build().execute(&self.pool).await;

            match output {
                Ok(_) => Ok(()),
                Err(err) => Err(err.into()),
            }
        };

        output
    }
}
