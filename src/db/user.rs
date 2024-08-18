use chrono::Local;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, QueryBuilder};

use super::DBConnection;

#[derive(Debug, sqlx::Type, Serialize, Deserialize, Clone)]
#[sqlx(type_name = "user_type", rename_all = "lowercase")]
pub enum UserType {
    User,
    Admin,
}

#[derive(FromRow, Clone, Debug)]
pub struct User {
    pub id: sqlx::types::Uuid,
    pub account_type: UserType,
    pub email: String,
    pub created_at: chrono::DateTime<Local>,
    pub updated_at: chrono::DateTime<Local>,
}

pub struct UserDatabase<'a> {
    db: &'a DBConnection,
}

impl<'a> UserDatabase<'a> {
    pub fn new(db: &'a DBConnection) -> UserDatabase {
        UserDatabase { db }
    }

    pub async fn fetch(&self, id: &str) -> anyhow::Result<Option<User>, sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("SELECT * FROM users WHERE id = "));

        query_builder.push_bind(id);

        let resp = query_builder
            .build_query_as::<User>()
            .fetch_optional(&self.db.db)
            .await?;

        Ok(resp)
    }

    pub async fn fetch_by_email(&self, email: &str) -> anyhow::Result<Option<User>, sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("SELECT * FROM users WHERE email = "));

        query_builder.push_bind(email);

        let resp = query_builder
            .build_query_as::<User>()
            .fetch_optional(&self.db.db)
            .await?;

        Ok(resp)
    }

    pub async fn insert(&self, email: &str) -> Result<(), sqlx::Error> {
        let mut builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("INSERT INTO users(email) VALUES ("));
        builder.push_bind(email);
        builder.push(")");

        let _ = builder.build().execute(&self.db.db).await?;

        Ok(())
    }
}
