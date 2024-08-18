use actix_session::Session;
use actix_web::{Error, get, HttpResponse, web};
use actix_web::error::{ErrorBadRequest, ErrorForbidden, ErrorInternalServerError};
use actix_web::web::Data;
use anyhow::format_err;
use oauth2::{
    AuthorizationCode, Client, CsrfToken, EndpointNotSet, EndpointSet, PkceCodeChallenge,
    PkceCodeVerifier, Scope, StandardRevocableToken, TokenResponse,
};
use oauth2::basic::{
    BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse,
    BasicTokenResponse,
};
use reqwest::ClientBuilder;
use serde::Deserialize;

use crate::db::DBConnection;
use crate::db::oauth::OAuthDatabase;
use crate::db::user::UserDatabase;

#[get("/auth")]
async fn auth_get(
    session: Session,
    oauth: Data<
        Option<
            Client<
                BasicErrorResponse,
                BasicTokenResponse,
                BasicTokenIntrospectionResponse,
                StandardRevocableToken,
                BasicRevocationErrorResponse,
                EndpointSet,
                EndpointNotSet,
                EndpointNotSet,
                EndpointSet,
                EndpointSet,
            >,
        >,
    >,
) -> Result<HttpResponse, Error> {
    let oauth = match oauth.as_ref() {
        Some(client) => client,
        None => return Err(ErrorInternalServerError("Auth Disabled")),
    };

    let (pkce_code_challenge, pkce_code_verifier) = PkceCodeChallenge::new_random_sha256();
    match session.insert("verifier", pkce_code_verifier.secret()) {
        Ok(_) => (),
        Err(e) => {
            return Err(ErrorInternalServerError(format_err!(
                "Failed to set verifier: {}",
                e
            )))
        }
    }

    let (authorise_url, _) = oauth
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("email".to_string()))
        .set_pkce_challenge(pkce_code_challenge)
        .url();

    let output = HttpResponse::Found()
        .insert_header(("Location", authorise_url.as_str()))
        .finish();

    Ok(output)
}

#[derive(Deserialize, Clone)]
struct AuthCallbackQuery {
    code: String,
    state: String,
}

#[get("/auth_callback")]
async fn auth_callback(
    session: Session,
    db: Data<DBConnection>,
    oauth: Data<
        Option<
            Client<
                BasicErrorResponse,
                BasicTokenResponse,
                BasicTokenIntrospectionResponse,
                StandardRevocableToken,
                BasicRevocationErrorResponse,
                EndpointSet,
                EndpointNotSet,
                EndpointNotSet,
                EndpointSet,
                EndpointSet,
            >,
        >,
    >,
    query: web::Query<AuthCallbackQuery>,
) -> Result<HttpResponse, Error> {
    let oauth = match oauth.as_ref() {
        Some(oauth_client) => oauth_client,
        None => return Err(ErrorInternalServerError("Auth disabled.")),
    };

    let secret = match session.get::<String>("verifier") {
        Ok(secret) => secret,
        Err(e) => return Err(ErrorBadRequest("Missing verifier, auth first")),
    };
    let secret = match secret {
        Some(secret) if secret.len() > 0 => secret,
        _ => return Err(ErrorBadRequest("Missing verifier, auth first")),
    };
    let verifier = PkceCodeVerifier::new(secret);

    let http_client = ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let token_response = oauth
        .exchange_code(AuthorizationCode::new((&query.code).clone()))
        .set_pkce_verifier(verifier)
        .request_async(&http_client)
        .await;

    return match token_response {
        Ok(token) => {
            let email =
                match GoogleV3UserInfo::request_own_email(token.access_token().secret()).await {
                    Ok(email) => email,
                    Err(err) => return Err(ErrorInternalServerError(err)),
                };

            let oauth_db = OAuthDatabase::new(&db);
            let user_db = UserDatabase::new(&db);
            match user_db.fetch_by_email(&email).await {
                Ok(user) => match user {
                    Some(user) => {
                        session.insert("email", user.email).unwrap();

                        let scopes = token
                            .scopes()
                            .unwrap()
                            .iter()
                            .map(|scope| scope.to_string())
                            .collect::<Vec<String>>();

                        let refresh_token = match token.refresh_token() {
                            Some(token) => token.secret(),
                            None => "None",
                        };

                        match oauth_db
                            .insert(
                                token.access_token().secret(),
                                refresh_token,
                                &scopes,
                                &email,
                            )
                            .await
                        {
                            Ok(_) => (),
                            Err(err) => {
                                return Err(ErrorInternalServerError(format_err!(
                                    "Failed to insert tokens: {}",
                                    err
                                )))
                            }
                        }
                        Ok(HttpResponse::Found()
                            .insert_header(("Location", "/"))
                            .finish())
                    }
                    None => Err(ErrorForbidden("Not an authorised user")),
                },
                Err(err) => Err(ErrorInternalServerError(format_err!(
                    "Failed to retrieve user: {}",
                    err
                ))),
            }
        }
        Err(err_token) => Err(ErrorInternalServerError("Failed to retrieve token")),
    };
}

#[derive(Deserialize, Clone, Debug)]
struct GoogleV3UserInfo {
    sub: String,
    picture: String,
    email: String,
    email_verified: bool,
}

impl GoogleV3UserInfo {
    pub async fn request_own_email(access_token: &str) -> anyhow::Result<String> {
        let client = ClientBuilder::new().build()?;
        let response = client
            .get("https://www.googleapis.com/oauth2/v3/userinfo")
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await?;

        if response.status().is_server_error() || response.status().is_client_error() {
            return Err(format_err!(
                "Failed to request email address: {}",
                response.status()
            ));
        }

        let data = response.bytes().await?;
        let data: Self = serde_json::from_slice(&data)?;

        let data = data.email;
        Ok(data)
    }
}
