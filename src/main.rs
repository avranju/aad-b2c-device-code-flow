#![warn(missing_debug_implementations, unsafe_code)]
#![deny(rust_2018_idioms, warnings)]

use std::{
    collections::{hash_map::Entry, HashMap},
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::Result;
use axum::{
    extract::{Form, Query},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::{get, get_service, post},
    Extension, Json, Router,
};
use azuread::{AuthorizeContext, AzureAd};
use error::AppError;
use oauth2::basic::BasicTokenResponse;
use serde::{Deserialize, Serialize};
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::trace;
use url::Url;

mod azuread;
mod error;
mod utils;

const DEFAULT_LISTEN_URL: &str = "0.0.0.0:32468";
const DEVICE_CODE_EXPIRY_IN_SECS: u64 = 60 * 5;
const DEVICE_CODE_GC_INTERVAL_IN_SECS: u64 = 60 * 2;

#[derive(Deserialize, Clone, Debug)]
struct Config {
    client_id: String,
    client_secret: String,
    tenant_name: String,
    policy_name: String,
    site_url: Url,
    code_length: usize,
    listen_url: Option<String>,
    scopes: Option<String>,
}

#[derive(Clone, Debug)]
struct CodeEntry {
    token: Option<BasicTokenResponse>,
    auth_context: Option<AuthorizeContext>,
    created_ts: Instant,
}

#[derive(Clone, Debug)]
enum CodeTokenStatus {
    Invalid,
    Pending,
    Complete(BasicTokenResponse),
}

#[derive(Clone, Debug)]
struct State {
    azure_ad: AzureAd,
    site_url: Url,
    code_length: usize,
    code_map: Arc<Mutex<HashMap<String, CodeEntry>>>,
}

impl State {
    fn new(config: Config) -> Result<Self> {
        let azure_ad = AzureAd::new(
            config.client_id,
            config.client_secret,
            config.tenant_name,
            config.policy_name,
            config.site_url.join("/auth/callback")?,
            config
                .scopes
                .map(|s| s.split(' ').map(String::from).collect())
                .unwrap_or_default(),
        )?;

        Ok(Self {
            azure_ad,
            site_url: config.site_url,
            code_length: config.code_length,
            code_map: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn garbage_collect(&mut self) {
        let mut code_map = self.code_map.lock().unwrap();

        // remove all expired items by retaining only non-expired items
        code_map.retain(|_, e| e.created_ts.elapsed().as_secs() < DEVICE_CODE_EXPIRY_IN_SECS);
    }

    fn add_new_code(&mut self) -> String {
        let mut code_map = self.code_map.lock().unwrap();

        // generate a unique unused device code
        let mut code = utils::generate_random_string(self.code_length);
        while code_map.contains_key(&code) {
            code = utils::generate_random_string(self.code_length);
        }

        code_map.insert(
            code.clone(),
            CodeEntry {
                token: None,
                auth_context: None,
                created_ts: Instant::now(),
            },
        );

        code
    }

    fn set_code_token(&mut self, code: String, token: BasicTokenResponse) -> bool {
        match self
            .code_map
            .lock()
            .unwrap()
            .entry(code)
            .and_modify(|e| e.token = Some(token))
        {
            Entry::Occupied(_) => true,
            Entry::Vacant(_) => false,
        }
    }

    fn get_code_token(&self, code: String) -> CodeTokenStatus {
        match self.code_map.lock().unwrap().get(&code) {
            Some(e) => match e.token.as_ref() {
                Some(t) => CodeTokenStatus::Complete(t.clone()),
                None => CodeTokenStatus::Pending,
            },
            None => CodeTokenStatus::Invalid,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // install global collector configured based on RUST_LOG env var.
    tracing_subscriber::fmt::init();

    let config = envy::prefixed("AADB2C_DEVICE_CODE_").from_env::<Config>()?;
    trace!("{config:#?}");

    let addr = config
        .listen_url
        .as_deref()
        .unwrap_or(DEFAULT_LISTEN_URL)
        .parse()?;

    let state = State::new(config)?;

    let app = Router::new()
        .route("/", get(|| async { Redirect::to("/device.html") }))
        .route("/code", get(generate_code))
        .route("/login", post(login))
        .route("/auth/callback", get(auth_callback))
        .route("/poll-token", get(poll_token))
        .fallback(get_service(ServeDir::new("www")).handle_error(handle_error))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(Extension(state.clone())),
        );

    // kick-off garbage collection for expired device codes
    tokio::spawn(run_code_gc(state));

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

async fn handle_error(_err: ::std::io::Error) -> impl IntoResponse {
    (StatusCode::INTERNAL_SERVER_ERROR, "I/O error")
}

async fn run_code_gc(mut state: State) {
    loop {
        state.garbage_collect();
        tokio::time::sleep(std::time::Duration::from_secs(
            DEVICE_CODE_GC_INTERVAL_IN_SECS,
        ))
        .await;
    }
}

#[derive(Debug, Serialize, Clone)]
struct CodeResponse {
    code: String,
    url: Url,
}

async fn generate_code(
    Extension(mut state): Extension<State>,
) -> Result<Json<CodeResponse>, AppError<url::ParseError>> {
    let code = state.add_new_code();

    Ok(Json(CodeResponse {
        code,
        url: state.site_url.join("/device.html")?,
    }))
}

#[derive(Deserialize)]
struct LoginForm {
    #[serde(rename = "device-code")]
    device_code: String,
}

async fn login(Extension(mut state): Extension<State>, Form(login): Form<LoginForm>) -> Redirect {
    state
        .code_map
        .lock()
        .unwrap()
        .get_mut(&login.device_code)
        .map(|entry| {
            // create authorization context
            let auth_context = state.azure_ad.create_authorize_context();
            let redirect_url = auth_context.authorize_url.as_str().to_string();
            entry.auth_context = Some(auth_context);

            // redirect to Azure AD to get the user to sign in
            Redirect::to(&redirect_url)
        })
        .unwrap_or_else(|| {
            // if we don't have an entry for this code, redirect to the login page
            Redirect::to("/device.html?error=invalid_code")
        })
}

#[derive(Deserialize)]
struct AuthResponse {
    state: String,
    code: String,
}

async fn auth_callback(
    Extension(mut state): Extension<State>,
    Query(auth_response): Query<AuthResponse>,
) -> Redirect {
    // if there's no state or code, we can't do anything
    if auth_response.state.is_empty() || auth_response.code.is_empty() {
        return Redirect::to("/device.html?error=invalid_response");
    }

    // look for a code map entry which has this csrf token in it
    let code_entry = state
        .code_map
        .lock()
        .unwrap()
        .iter_mut()
        .find(|(_, e)| {
            e.auth_context
                .as_ref()
                .map(|c| *c.csrf_token.secret() == auth_response.state)
                .is_some()
        })
        .map(|(device_code, code_entry)| {
            (
                device_code.clone(),
                // The "expect" call below won't panic because:
                //  1. We have a lock on "code_map"
                //  2. We already checked that this entry exists and the csrf token
                //     matches which wouldn't have passed if this was None.
                code_entry
                    .auth_context
                    .take()
                    .expect("Auth context should not be None."),
            )
        });

    if let Some((device_code, auth_context)) = code_entry {
        let res = state
            .azure_ad
            .exchange_code(auth_response.code, &auth_context)
            .await;

        if let Ok(token) = res {
            state.set_code_token(device_code, token);
            Redirect::to("/complete.html")
        } else {
            Redirect::to("/device.html?error=auth_failed")
        }
    } else {
        Redirect::to("/device.html?error=invalid_response")
    }
}

#[derive(Deserialize)]
struct PollDeviceCode {
    code: String,
}

async fn poll_token(
    Extension(state): Extension<State>,
    Query(poll_code): Query<PollDeviceCode>,
) -> Result<Json<BasicTokenResponse>, StatusCode> {
    match state.get_code_token(poll_code.code) {
        CodeTokenStatus::Invalid => Err(StatusCode::NOT_FOUND),
        CodeTokenStatus::Pending => Err(StatusCode::NO_CONTENT),
        CodeTokenStatus::Complete(token) => Ok(Json(token)),
    }
}
