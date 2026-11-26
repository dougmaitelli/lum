//! Optional authentication for the admin listener only. Tokens never reach the browser.
use anyhow::{Context, Result, ensure};
use axum::{
    Extension, Router,
    extract::{Query, Request},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default, Clone)]
pub struct Settings {
    pub enabled: bool,
    issuer: String,
    pub origin: String,
    client_id: String,
    allowed_subjects: String,
    client_secret_file: String,
}
impl Settings {
    pub fn from_env() -> Result<Self> {
        fn read(key: &str) -> Result<Option<String>> {
            match std::env::var(key) {
                Ok(value) => Ok(Some(value.trim().to_owned())),
                Err(std::env::VarError::NotPresent) => Ok(None),
                Err(_) => anyhow::bail!("{key} must be valid UTF-8"),
            }
        }
        let flag = read("LUM_OIDC_ENABLED")?;
        let cfg = Self {
            enabled: flag
                .as_deref()
                .unwrap_or("false")
                .parse()
                .context("LUM_OIDC_ENABLED must be true or false")?,
            issuer: read("LUM_OIDC_ISSUER")?.unwrap_or_default(),
            origin: read("LUM_OIDC_ORIGIN")?.unwrap_or_default(),
            client_id: read("LUM_OIDC_CLIENT_ID")?.unwrap_or_default(),
            allowed_subjects: read("LUM_OIDC_ALLOWED_SUBJECTS")?.unwrap_or_default(),
            client_secret_file: read("LUM_OIDC_CLIENT_SECRET_FILE")?.unwrap_or_default(),
        };
        ensure!(
            flag.is_some()
                || [
                    &cfg.issuer,
                    &cfg.origin,
                    &cfg.client_id,
                    &cfg.allowed_subjects,
                    &cfg.client_secret_file
                ]
                .iter()
                .all(|v| v.is_empty()),
            "Set LUM_OIDC_ENABLED explicitly when configuring OIDC"
        );
        cfg.validate()?;
        Ok(cfg)
    }
    pub fn validate(&self) -> Result<()> {
        for (s, origin) in [(&self.origin, true), (&self.issuer, false)] {
            if !s.is_empty() {
                let u = reqwest::Url::parse(s).context("Invalid OIDC URL")?;
                ensure!(
                    u.scheme() == "https"
                        && u.host().is_some()
                        && u.username().is_empty()
                        && u.password().is_none()
                        && u.query().is_none()
                        && u.fragment().is_none(),
                    "OIDC URLs require HTTPS without credentials, query or fragment"
                );
                if origin {
                    ensure!(
                        u.origin().ascii_serialization() == *s,
                        "LUM_OIDC_ORIGIN must be a canonical HTTPS origin without a trailing slash"
                    );
                }
            }
        }
        ensure!(
            self.client_secret_file.is_empty() || Path::new(&self.client_secret_file).is_absolute(),
            "Secret file path must be absolute"
        );
        ensure!(
            !self.enabled
                || (!self.issuer.is_empty()
                    && !self.origin.is_empty()
                    && !self.client_id.trim().is_empty()
                    && self.subjects().next().is_some()),
            "Set LUM_OIDC_ISSUER, LUM_OIDC_ORIGIN, LUM_OIDC_CLIENT_ID and LUM_OIDC_ALLOWED_SUBJECTS before enabling OIDC"
        );
        Ok(())
    }
    fn subjects(&self) -> impl Iterator<Item = &str> {
        self.allowed_subjects
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}
#[cfg(test)]
mod tests;
const FLOW: &str = "__Host-lum-flow";
const SESSION: &str = "__Host-lum-session";
const LOGIN_TTL: Duration = Duration::from_secs(300);
const SESSION_TTL: Duration = Duration::from_secs(8 * 3600);
struct Flow {
    state: String,
    nonce: Nonce,
    verifier: PkceCodeVerifier,
    provider: Arc<CoreProviderMetadata>,
}
struct Entries<T>(HashMap<String, (Instant, T)>);
impl<T> Entries<T> {
    fn insert(&mut self, value: T, ttl: Duration) -> Option<String> {
        let now = Instant::now();
        self.0.retain(|_, (end, _)| *end > now);
        if self.0.len() >= 1024 {
            return None;
        }
        let id = CsrfToken::new_random().secret().clone();
        self.0.insert(id.clone(), (now + ttl, value));
        Some(id)
    }
    fn take(&mut self, id: &str) -> Option<T> {
        self.0
            .remove(id)
            .filter(|(end, _)| *end > Instant::now())
            .map(|(_, v)| v)
    }
    fn valid(&self, id: &str) -> bool {
        self.0.get(id).is_some_and(|(end, _)| *end > Instant::now())
    }
}
struct Auth {
    cfg: Settings,
    secret: Option<ClientSecret>,
    http: reqwest::Client,
    provider: tokio::sync::Mutex<Option<(Instant, Arc<CoreProviderMetadata>)>>,
    flows: Mutex<Entries<Flow>>,
    sessions: Mutex<Entries<()>>,
}
impl Auth {
    async fn provider(&self) -> Result<Arc<CoreProviderMetadata>> {
        let mut cache = self.provider.lock().await;
        if let Some((end, p)) = &*cache
            && *end > Instant::now()
        {
            return Ok(p.clone());
        }
        let p = CoreProviderMetadata::discover_async(
            IssuerUrl::new(self.cfg.issuer.clone())?,
            &self.http,
        )
        .await
        .map_err(|_| anyhow::anyhow!("Discovery unavailable"))?;
        ensure!(
            p.authorization_endpoint().url().scheme() == "https"
                && p.token_endpoint()
                    .is_some_and(|u| u.url().scheme() == "https"),
            "Invalid provider endpoints"
        );
        let p = Arc::new(p);
        *cache = Some((Instant::now() + LOGIN_TTL, p.clone()));
        Ok(p)
    }
    fn client(
        &self,
        p: &CoreProviderMetadata,
    ) -> openidconnect::core::CoreClient<
        openidconnect::EndpointSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointMaybeSet,
        openidconnect::EndpointMaybeSet,
    > {
        CoreClient::from_provider_metadata(
            p.clone(),
            ClientId::new(self.cfg.client_id.clone()),
            self.secret.clone(),
        )
        .set_redirect_uri(
            RedirectUrl::new(format!("{}/auth/callback", self.cfg.origin))
                .expect("validated origin"),
        )
    }
}
fn cookie<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = h
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|s| s.split(';'))
        .filter_map(|s| s.trim().split_once('='))
        .filter(|(k, _)| *k == name);
    let (_, v) = values.next()?;
    values.next().is_none().then_some(v)
}
fn set_cookie(r: &mut Response, name: &str, value: &str, seconds: u64) {
    r.headers_mut().append(
        header::SET_COOKIE,
        format!("{name}={value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={seconds}")
            .parse()
            .unwrap(),
    );
}
pub fn protect<S: Clone + Send + Sync + 'static>(
    router: Router<S>,
    cfg: &Settings,
) -> Result<Router<S>> {
    if !cfg.enabled {
        return Ok(router);
    }
    cfg.validate()?;
    let secret = if cfg.client_secret_file.is_empty() {
        None
    } else {
        let s = std::fs::read_to_string(&cfg.client_secret_file)
            .context("Read LUM_OIDC_CLIENT_SECRET_FILE")?;
        ensure!(!s.trim().is_empty(), "OIDC secret file is empty");
        Some(ClientSecret::new(s.trim().to_owned()))
    };
    let auth = Arc::new(Auth {
        cfg: cfg.clone(),
        secret,
        http: reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()?,
        provider: Default::default(),
        flows: Mutex::new(Entries(HashMap::new())),
        sessions: Mutex::new(Entries(HashMap::new())),
    });
    Ok(routes(router, auth))
}
fn routes<S: Clone + Send + Sync + 'static>(router: Router<S>, auth: Arc<Auth>) -> Router<S> {
    router
        .route_layer(middleware::from_fn(guard))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
        .layer(Extension(auth))
}
async fn guard(Extension(a): Extension<Arc<Auth>>, req: Request, next: Next) -> Response {
    if cookie(req.headers(), SESSION).is_some_and(|id| a.sessions.lock().unwrap().valid(id)) {
        return next.run(req).await;
    }
    if req.uri().path() == "/"
        || req.uri().path() == "/api"
        || req.uri().path().starts_with("/chat/")
    {
        Redirect::to("/login").into_response()
    } else {
        (StatusCode::UNAUTHORIZED, "Sign in to Lum").into_response()
    }
}
async fn login(Extension(a): Extension<Arc<Auth>>) -> Response {
    let Ok(p) = a.provider().await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "OIDC provider unavailable. Check provider settings and retry.",
        )
            .into_response();
    };
    let client = a.client(&p);
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, state, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .set_pkce_challenge(challenge)
        .url();
    let Some(id) = a.flows.lock().unwrap().insert(
        Flow {
            state: state.secret().clone(),
            nonce,
            verifier,
            provider: p,
        },
        LOGIN_TTL,
    ) else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let mut r = Redirect::to(url.as_str()).into_response();
    set_cookie(&mut r, FLOW, &id, LOGIN_TTL.as_secs());
    r
}
#[derive(Deserialize)]
struct Callback {
    code: Option<String>,
    state: Option<String>,
}
async fn callback(
    Extension(a): Extension<Arc<Auth>>,
    h: HeaderMap,
    Query(q): Query<Callback>,
) -> Response {
    let mut r = match finish(&a, &h, q).await {
        Ok((id, ttl)) => {
            let mut r = Redirect::to("/").into_response();
            set_cookie(&mut r, SESSION, &id, ttl.as_secs());
            r
        }
        Err(()) => (
            StatusCode::FORBIDDEN,
            "Sign-in denied or expired. Return to /login to try again.",
        )
            .into_response(),
    };
    set_cookie(&mut r, FLOW, "", 0);
    r
}
async fn finish(a: &Auth, h: &HeaderMap, q: Callback) -> Result<(String, Duration), ()> {
    let flow = a
        .flows
        .lock()
        .unwrap()
        .take(cookie(h, FLOW).ok_or(())?)
        .ok_or(())?;
    if q.state.as_ref() != Some(&flow.state) {
        return Err(());
    }
    let client = a.client(&flow.provider);
    let tokens = client
        .exchange_code(AuthorizationCode::new(q.code.ok_or(())?))
        .map_err(|_| ())?
        .set_pkce_verifier(flow.verifier)
        .request_async(&a.http)
        .await
        .map_err(|_| ())?;
    use openidconnect::{AccessTokenHash, OAuth2TokenResponse};
    let id_token = tokens.id_token().ok_or(())?;
    let verifier = client.id_token_verifier();
    let claims = id_token.claims(&verifier, &flow.nonce).map_err(|_| ())?;
    if let Some(expected) = claims.access_token_hash() {
        let actual = AccessTokenHash::from_token(
            tokens.access_token(),
            id_token.signing_alg().map_err(|_| ())?,
            id_token.signing_key(&verifier).map_err(|_| ())?,
        )
        .map_err(|_| ())?;
        if &actual != expected {
            return Err(());
        }
    }
    if !a.cfg.subjects().any(|s| s == claims.subject().as_str())
        || claims
            .authorized_party()
            .is_some_and(|azp| azp.as_str() != a.cfg.client_id)
    {
        return Err(());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ())?
        .as_secs();
    let ttl = SESSION_TTL.min(Duration::from_secs(
        (claims.expiration().timestamp().max(0) as u64).saturating_sub(now),
    ));
    if ttl.is_zero() {
        return Err(());
    }
    let id = a.sessions.lock().unwrap().insert((), ttl).ok_or(())?;
    Ok((id, ttl))
}
async fn logout(Extension(a): Extension<Arc<Auth>>, h: HeaderMap) -> Response {
    if let Some(id) = cookie(&h, SESSION) {
        a.sessions.lock().unwrap().take(id);
    }
    let mut r = Redirect::to("/login").into_response();
    set_cookie(&mut r, SESSION, "", 0);
    r
}
