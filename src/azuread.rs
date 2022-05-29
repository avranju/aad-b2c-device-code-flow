use anyhow::Result;
use oauth2::{
    basic::{BasicClient, BasicTokenResponse},
    reqwest::async_http_client,
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenUrl,
};
use url::Url;

#[derive(Debug)]
pub struct AuthorizeContext {
    pub pkce_code_verifier: PkceCodeVerifier,
    pub csrf_state: CsrfToken,
    pub authorize_url: Url,
}

#[derive(Debug, Clone)]
pub struct AzureAd {
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
    pub redirect_url: Url,
    pub auth_url: AuthUrl,
    pub token_url: TokenUrl,
    pub scopes: Vec<&'static str>,
}

impl AzureAd {
    pub fn new(
        client_id: String,
        client_secret: String,
        tenant_name: String,
        policy_name: String,
        redirect_url: Url,
        scopes: Vec<&'static str>,
    ) -> Result<Self> {
        let client_id = ClientId::new(client_id);
        let client_secret = ClientSecret::new(client_secret);
        let auth_url = oauth2::AuthUrl::from_url(Url::parse(&format!(
            "https://{}.b2clogin.com/{}.onmicrosoft.com/{}/oauth2/v2.0/authorize",
            tenant_name, tenant_name, policy_name
        ))?);
        let token_url = oauth2::TokenUrl::from_url(Url::parse(&format!(
            "https://{}.b2clogin.com/{}.onmicrosoft.com/{}/oauth2/v2.0/token",
            tenant_name, tenant_name, policy_name
        ))?);

        Ok(Self {
            client_id,
            client_secret,
            redirect_url,
            auth_url,
            token_url,
            scopes,
        })
    }

    pub fn create_authorize_url(&mut self) -> AuthorizeContext {
        let (pkce_code_challenge, pkce_code_verifier) = PkceCodeChallenge::new_random_sha256();

        let client = BasicClient::new(
            self.client_id.clone(),
            Some(self.client_secret.clone()),
            self.auth_url.clone(),
            Some(self.token_url.clone()),
        )
        .set_auth_type(AuthType::RequestBody)
        .set_redirect_uri(RedirectUrl::from_url(self.redirect_url.clone()));

        let (authorize_url, csrf_state) = client
            .authorize_url(oauth2::CsrfToken::new_random)
            .add_scopes(self.scopes.iter().map(|s| Scope::new(s.to_string())))
            .set_pkce_challenge(pkce_code_challenge.clone())
            .url();

        AuthorizeContext {
            pkce_code_verifier,
            csrf_state,
            authorize_url,
        }
    }

    pub async fn exchange_code(
        &self,
        code: String,
        context: AuthorizeContext,
    ) -> Result<BasicTokenResponse> {
        let client = BasicClient::new(
            self.client_id.clone(),
            None,
            self.auth_url.clone(),
            Some(self.token_url.clone()),
        )
        .set_auth_type(AuthType::RequestBody);

        let scopes_str = self.scopes.join(" ").to_string();

        Ok(client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(context.pkce_code_verifier)
            .add_extra_param("scope", scopes_str)
            .request_async(async_http_client)
            .await?)
    }
}
