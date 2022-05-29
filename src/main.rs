#![warn(missing_debug_implementations, unsafe_code)]
// #![deny(rust_2018_idioms, warnings)]

use std::{
    collections::{hash_map::Entry, HashMap},
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::Result;
use axum::{
    extract::Form,
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::{get, get_service, post},
    Extension, Json, Router,
};
use azuread::AzureAd;
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
const DEVICE_CODE_EXPIRY_IN_SECS: u64 = 60 * 2;

#[derive(Deserialize, Clone, Debug)]
struct Config {
    client_id: String,
    client_secret: String,
    tenant_name: String,
    policy_name: String,
    site_url: Url,
    code_length: usize,
    listen_url: Option<String>,
}

#[derive(Clone, Debug)]
struct CodeEntry {
    token: Option<BasicTokenResponse>,
    created_ts: Instant,
}

#[derive(Clone, Debug)]
struct State {
    azure_ad: AzureAd,
    site_url: Url,
    code_length: usize,
    listen_url: Option<String>,
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
            [
                "https://nworksad.onmicrosoft.com/toteup-api/Items.Upload",
                "https://nworksad.onmicrosoft.com/toteup-api/Items.List",
            ]
            .into_iter()
            .collect(),
        )?;

        Ok(Self {
            azure_ad,
            site_url: config.site_url,
            code_length: config.code_length,
            listen_url: config.listen_url,
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
                created_ts: Instant::now(),
            },
        );

        code
    }

    fn set_code_token(&mut self, code: String, token: BasicTokenResponse) -> bool {
        let mut code_map = self.code_map.lock().unwrap();

        match code_map.entry(code).and_modify(|e| e.token = Some(token)) {
            Entry::Occupied(_) => true,
            Entry::Vacant(_) => false,
        }
    }

    fn get_code_token(&self, code: String) -> Option<BasicTokenResponse> {
        self.code_map
            .lock()
            .unwrap()
            .get(&code)
            .and_then(|e| e.token.as_ref())
            .cloned()
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
        .fallback(get_service(ServeDir::new("www")).handle_error(handle_error))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(Extension(state)),
        );

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

async fn handle_error(_err: ::std::io::Error) -> impl IntoResponse {
    (StatusCode::INTERNAL_SERVER_ERROR, "I/O error")
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

async fn login(
    Extension(_state): Extension<State>,
    Form(login): Form<LoginForm>,
) -> Result<Redirect, AppError<String>> {
    Ok(Redirect::to("https://musings.nerdworks.dev"))
}

async fn auth_callback(Extension(state): Extension<State>) -> Result<(), AppError<String>> {
    Ok(())
}
