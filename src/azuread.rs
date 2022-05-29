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
    pub csrf_token: CsrfToken,
    pub authorize_url: Url,
}

impl Clone for AuthorizeContext {
    fn clone(&self) -> Self {
        Self {
            pkce_code_verifier: PkceCodeVerifier::new(self.pkce_code_verifier.secret().clone()),
            csrf_token: self.csrf_token.clone(),
            authorize_url: self.authorize_url.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AzureAd {
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
    pub redirect_url: Url,
    pub auth_url: AuthUrl,
    pub token_url: TokenUrl,
    pub scopes: Vec<String>,
}

impl AzureAd {
    pub fn new(
        client_id: String,
        client_secret: String,
        tenant_name: String,
        policy_name: String,
        redirect_url: Url,
        scopes: Vec<String>,
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

    pub fn create_authorize_context(&mut self) -> AuthorizeContext {
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
            .add_scopes(self.scopes.iter().map(|s| Scope::new(s.clone())))
            .set_pkce_challenge(pkce_code_challenge)
            .url();

        AuthorizeContext {
            pkce_code_verifier,
            csrf_token: csrf_state,
            authorize_url,
        }
    }

    pub async fn exchange_code(
        &self,
        code: String,
        context: &AuthorizeContext,
    ) -> Result<BasicTokenResponse> {
        let client = BasicClient::new(
            self.client_id.clone(),
            None,
            self.auth_url.clone(),
            Some(self.token_url.clone()),
        )
        .set_auth_type(AuthType::RequestBody);

        let scopes_str = self.scopes.join(" ");

        Ok(client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(PkceCodeVerifier::new(
                context.pkce_code_verifier.secret().clone(),
            ))
            .add_extra_param("scope", scopes_str)
            .request_async(async_http_client)
            .await?)
    }
}
