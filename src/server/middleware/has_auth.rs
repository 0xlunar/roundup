use std::future::{ready, Ready};

use actix_session::SessionExt;
use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpResponse,
};
use actix_web::body::EitherBody;
use actix_web::web::Data;
use futures_util::future::LocalBoxFuture;
use log::error;
use oauth2::{Client, EndpointNotSet, EndpointSet, StandardRevocableToken};
use oauth2::basic::{
    BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse,
    BasicTokenResponse,
};

use crate::db::DBConnection;
use crate::db::user::{UserDatabase, UserType};

pub struct HasAuthorisation;

impl<S, B> Transform<S, ServiceRequest> for HasAuthorisation
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Transform = HasAuthorisationMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(HasAuthorisationMiddleware { service }))
    }
}

pub struct HasAuthorisationMiddleware<S> {
    service: S,
}

// This future doesn't have the requirement of being `Send`.
// See: futures_util::future::LocalBoxFuture
// type LocalBoxFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

// `S`: type of the wrapped service
// `B`: type of the body - try to be generic over the body where possible
impl<S, B> Service<ServiceRequest> for HasAuthorisationMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let oauth_client = req.app_data::<Data<
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
        >>();
        if req.path() == "/favicon.ico" {
            let fut = self.service.call(req);
            return Box::pin(async move {
                let res = fut.await?;
                Ok(res.map_into_left_body())
            });
        }

        match oauth_client {
            Some(client) => match client.get_ref() {
                Some(_) => (),
                None => {
                    // Auth Disabled
                    let fut = self.service.call(req);
                    return Box::pin(async move {
                        let res = fut.await?;
                        Ok(res.map_into_left_body())
                    });
                }
            },
            None => {
                error!("Missing OAuth client, assuming no authentication");
                // hopefully shouldn't reach here but assume auth is disabled?
                let fut = self.service.call(req);
                return Box::pin(async move {
                    let res = fut.await?;
                    Ok(res.map_into_left_body())
                });
            }
        };

        let has_auth = has_auth(&req);
        if has_auth {
            if req.path() == "/auth" || req.path() == "/auth_callback" {
                let res = HttpResponse::Found()
                    .insert_header(("Location", "/"))
                    .finish();
                let (http_req, _) = req.into_parts();
                let res = ServiceResponse::new(http_req, res);
                Box::pin(async move { Ok(res.map_into_right_body()) })
            } else {
                let fut = self.service.call(req);
                Box::pin(async move {
                    let res = fut.await?;
                    Ok(res.map_into_left_body())
                })
            }
        } else {
            if req.path() == "/auth" || req.path() == "/auth_callback" {
                let fut = self.service.call(req);
                Box::pin(async move {
                    let res = fut.await?;
                    Ok(res.map_into_left_body())
                })
            } else {
                let res = HttpResponse::Found()
                    .insert_header(("Location", "/auth"))
                    .finish();
                let (http_req, _) = req.into_parts();
                let res = ServiceResponse::new(http_req, res);
                Box::pin(async move { Ok(res.map_into_right_body()) })
            }
        }
    }
}

fn has_auth(req: &ServiceRequest) -> bool {
    let session = req.get_session();
    let email = match session.get::<String>("email") {
        Ok(email) => email,
        Err(err) => {
            error!("{}", err);
            return false;
        }
    };
    match email {
        Some(email) => {
            let db = req.app_data::<Data<DBConnection>>().unwrap();
            let user_db = UserDatabase::new(db);

            let db_result =
                futures::executor::block_on(async { user_db.fetch_by_email(&email).await });
            let db_result = match db_result {
                Ok(user) => user,
                Err(_) => return false,
            };

            match db_result {
                Some(_) => true,
                None => false,
            }
        }
        None => false,
    }
}

fn is_admin(req: &ServiceRequest) -> bool {
    let session = req.get_session();
    let email = match session.get::<String>("email") {
        Ok(email) => email,
        Err(err) => {
            error!("{}", err);
            return false;
        }
    };
    match email {
        Some(token) => {
            let db = req.app_data::<Data<DBConnection>>().unwrap();
            let user_db = UserDatabase::new(db);

            let db_result =
                futures::executor::block_on(async { user_db.fetch_by_email(&token).await });
            let db_result = match db_result {
                Ok(user) => user,
                Err(_) => return false,
            };

            match db_result {
                Some(user) => match user.account_type {
                    UserType::User => false,
                    UserType::Admin => true,
                },
                None => false,
            }
        }
        None => false,
    }
}
