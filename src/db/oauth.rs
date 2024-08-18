use chrono::Local;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, QueryBuilder};

use super::DBConnection;

#[derive(FromRow, Deserialize, Serialize, Clone, Debug)]
pub struct OAuth2Item {
    pub access_token: String,
    pub refresh_token: String,
    pub scope: Vec<String>,
    pub email: String,
    pub created_at: chrono::DateTime<Local>,
    pub updated_at: chrono::DateTime<Local>,
}

pub struct OAuthDatabase<'a> {
    db: &'a DBConnection,
}

impl<'a> OAuthDatabase<'a> {
    pub fn new(db: &'a DBConnection) -> OAuthDatabase {
        OAuthDatabase { db }
    }

    pub async fn fetch(
        &self,
        access_token: &str,
    ) -> anyhow::Result<Option<OAuth2Item>, sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("SELECT * FROM oauth WHERE access_token = "));

        query_builder.push_bind(access_token);

        let resp = query_builder
            .build_query_as::<OAuth2Item>()
            .fetch_optional(&self.db.db)
            .await?;

        Ok(resp)
    }

    pub async fn fetch_by_email(
        &self,
        email: &str,
    ) -> anyhow::Result<Option<OAuth2Item>, sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("SELECT * FROM oauth WHERE email = "));

        query_builder.push_bind(email);

        let resp = query_builder
            .build_query_as::<OAuth2Item>()
            .fetch_optional(&self.db.db)
            .await?;

        Ok(resp)
    }

    pub async fn insert(
        &self,
        access_token: &str,
        refresh_token: &str,
        scope: &[String],
        email: &str,
    ) -> Result<(), sqlx::Error> {
        let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(String::from(
            "INSERT INTO oauth(access_token, refresh_token, scope, email) VALUES (",
        ));
        builder.push_bind(access_token);

        builder.push(", ");
        builder.push_bind(refresh_token);

        builder.push(", ");
        builder.push_bind(scope);

        builder.push(", ");
        builder.push_bind(email);

        builder.push(")");

        let _ = builder.build().execute(&self.db.db).await?;

        Ok(())
    }

    pub async fn update(
        &self,
        email: &str,
        access_token: &str,
        refresh_token: &str,
        scope: Option<&[String]>,
    ) -> Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("UPDATE oauth SET access_token = "));
        query_builder.push_bind(access_token);
        query_builder.push(", refresh_token = ");
        query_builder.push_bind(refresh_token);

        match scope {
            Some(scope) => {
                query_builder.push(", scope = ");
                query_builder.push_bind(scope);
            }
            None => (),
        }

        query_builder.push(", updated_at = ");
        query_builder.push_bind(Local::now());

        query_builder.push(" WHERE email = ");
        query_builder.push_bind(email);

        let _ = query_builder.build().execute(&self.db.db).await?;

        Ok(())
    }

    pub async fn delete(&self, email: String) -> Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("DELETE FROM oauth WHERE email = "));
        query_builder.push_bind(email);

        let _ = query_builder.build().execute(&self.db.db).await?;

        Ok(())
    }
}
