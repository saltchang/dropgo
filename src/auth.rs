//! OIDC sign-in for the owner, on both hosts.

use axum::{
    extract::{Query, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use openidconnect::{
    AccessToken, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, OAuth2TokenResponse,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, UserInfoUrl,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use tower_sessions::{Expiry, Session};
use uuid::Uuid;

use crate::{AppState, i18n};

pub const SESSION_HOURS: i64 = 12;
const OWNER_KEY: &str = "owner";
const PENDING_KEY: &str = "oidc";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Host {
    Admin,
    Share,
}

/// Provider metadata is fetched on first use and kept for 10 minutes, so key rotation needs no restart
/// and the app starts while the provider is down.
pub struct Oidc {
    metadata: std::sync::Mutex<Option<(std::time::Instant, CoreProviderMetadata)>>,
    http: openidconnect::reqwest::Client,
}

impl Oidc {
    pub fn new() -> Self {
        let http = openidconnect::reqwest::Client::builder()
            .redirect(openidconnect::reqwest::redirect::Policy::none())
            .build()
            .expect("http client");
        Oidc { metadata: Default::default(), http }
    }
}

#[derive(Serialize, Deserialize)]
struct Owner {
    user_id: Uuid,
    until: i64,
}

#[derive(Serialize, Deserialize)]
struct Pending {
    csrf: String,
    nonce: String,
    verifier: String,
    return_to: String,
}

/// The signed-in owner of this session, if the session is younger than 12 hours.
pub async fn current_owner(session: &Session) -> Option<Uuid> {
    let owner: Owner = session.get(OWNER_KEY).await.ok().flatten()?;
    (owner.until > OffsetDateTime::now_utc().unix_timestamp()).then_some(owner.user_id)
}

/// Only a same-host path that starts with a single `/` is accepted; anything else becomes `/`.
pub fn safe_return(p: &str) -> String {
    let ok = p.starts_with('/')
        && !p.starts_with("//")
        && !p.starts_with("/\\")
        && !p.chars().any(|c| c.is_control() || c == '\\');
    if ok { p.to_string() } else { "/".to_string() }
}

async fn client(
    st: &AppState,
    host: Host,
) -> anyhow::Result<
    CoreClient<
        openidconnect::EndpointSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointMaybeSet,
        openidconnect::EndpointMaybeSet,
    >,
> {
    let oidc = &st.oidc;
    let cached = oidc
        .metadata
        .lock()
        .expect("oidc lock")
        .as_ref()
        .filter(|(at, _)| at.elapsed().as_secs() < 600)
        .map(|(_, m)| m.clone());
    let metadata = match cached {
        Some(m) => m,
        None => {
            let m =
                CoreProviderMetadata::discover_async(IssuerUrl::new(st.cfg.oidc_issuer.clone())?, &oidc.http).await?;
            *oidc.metadata.lock().expect("oidc lock") = Some((std::time::Instant::now(), m.clone()));
            m
        }
    };
    let redirect = match host {
        Host::Admin => format!("{}/auth/callback", st.cfg.admin_origin),
        Host::Share => format!("{}/_dropgo/auth/callback", st.cfg.share_origin),
    };
    Ok(CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(st.cfg.oidc_client_id.clone()),
        Some(ClientSecret::new(st.cfg.oidc_client_secret.clone())),
    )
    .set_redirect_uri(RedirectUrl::new(redirect)?))
}

/// Starts the code flow and remembers where to return in the session.
pub async fn start_login(st: &AppState, session: &Session, host: Host, return_to: &str) -> Response {
    let client = match client(st, host).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("OIDC discovery failed: {e:#}");
            return (StatusCode::BAD_GATEWAY, "Sign-in provider unavailable").into_response();
        }
    };
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, csrf, nonce) = client
        .authorize_url(CoreAuthenticationFlow::AuthorizationCode, CsrfToken::new_random, Nonce::new_random)
        .add_scope(Scope::new("profile".into()))
        .set_pkce_challenge(challenge)
        .url();
    let pending = Pending {
        csrf: csrf.secret().clone(),
        nonce: nonce.secret().clone(),
        verifier: verifier.secret().clone(),
        return_to: safe_return(return_to),
    };
    // A sign-in that never finishes leaves a row; it expires soon instead of after the default two weeks.
    session.set_expiry(Some(Expiry::OnInactivity(Duration::minutes(15))));
    if let Err(e) = session.insert(PENDING_KEY, pending).await {
        tracing::error!("session insert: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    (StatusCode::FOUND, [(header::LOCATION, url.to_string())]).into_response()
}

#[derive(Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: Option<String>,
}

pub async fn admin_callback(st: State<AppState>, s: Session, h: HeaderMap, q: Query<Callback>) -> Response {
    callback(st, s, h, q, Host::Admin).await
}

pub async fn share_callback(st: State<AppState>, s: Session, h: HeaderMap, q: Query<Callback>) -> Response {
    callback(st, s, h, q, Host::Share).await
}

async fn callback(
    State(st): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Query(q): Query<Callback>,
    host: Host,
) -> Response {
    match finish_login(&st, &session, &headers, q, host).await {
        Ok(Ok(return_to)) => Redirect::to(&return_to).into_response(),
        Ok(Err(status)) => {
            let t = st.i18n.t(i18n::negotiate(headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok())));
            let msg = if status == StatusCode::FORBIDDEN { t.s("auth.not_allowed") } else { t.s("auth.failed") };
            (status, msg.to_string()).into_response()
        }
        Err(e) => {
            tracing::warn!("sign-in failed: {e:#}");
            (StatusCode::BAD_REQUEST, "Sign-in failed").into_response()
        }
    }
}

async fn finish_login(
    st: &AppState,
    session: &Session,
    headers: &HeaderMap,
    q: Callback,
    host: Host,
) -> anyhow::Result<Result<String, StatusCode>> {
    let Some(pending) = session.remove::<Pending>(PENDING_KEY).await? else {
        return Ok(Err(StatusCode::BAD_REQUEST));
    };
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return Ok(Err(StatusCode::BAD_REQUEST));
    };
    if state != pending.csrf {
        return Ok(Err(StatusCode::BAD_REQUEST));
    }
    let client = client(st, host).await?;
    let token = client
        .exchange_code(AuthorizationCode::new(code))?
        .set_pkce_verifier(PkceCodeVerifier::new(pending.verifier))
        .request_async(&st.oidc.http)
        .await?;
    let id_token = token.id_token().ok_or_else(|| anyhow::anyhow!("no id_token"))?;
    let claims = id_token.claims(&client.id_token_verifier(), &Nonce::new(pending.nonce))?;
    let issuer = claims.issuer().as_str().to_string();
    let subject = claims.subject().as_str().to_string();

    // The token is verified above, so its payload can be read for any claim name.
    let payload = id_token.to_string();
    let payload = payload.split('.').nth(1).unwrap_or_default();
    let payload: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)?)?;
    let mut claim = payload.get(&st.cfg.owner_claim).and_then(|v| v.as_str()).map(str::to_string);
    // Some providers, such as Authelia 4.39+, leave profile claims out of the ID token by default.
    if claim.is_none() {
        claim = userinfo_claim(st, client.user_info_url(), token.access_token(), &subject).await?;
    }
    let claim = claim.unwrap_or_default();
    if claim.is_empty() || !st.cfg.owners.contains(&claim) {
        return Ok(Err(StatusCode::FORBIDDEN));
    }

    let locale = i18n::negotiate(headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()));
    let user_id = find_or_create_user(&st.db, &issuer, &subject, locale).await?;
    // API requests check this against the allowlist, since they have no session.
    sqlx::query("UPDATE users SET owner_claim = $2 WHERE id = $1").bind(user_id).bind(&claim).execute(&st.db).await?;

    let until = OffsetDateTime::now_utc() + Duration::hours(SESSION_HOURS);
    session.cycle_id().await?;
    session.insert(OWNER_KEY, Owner { user_id, until: until.unix_timestamp() }).await?;
    session.set_expiry(Some(Expiry::AtDateTime(until)));
    Ok(Ok(pending.return_to))
}

/// The owner claim from the userinfo endpoint, checked to be about the ID token's subject.
async fn userinfo_claim(
    st: &AppState,
    url: Option<&UserInfoUrl>,
    access: &AccessToken,
    subject: &str,
) -> anyhow::Result<Option<String>> {
    let Some(url) = url else { return Ok(None) };
    let res = st.oidc.http.get(url.url().clone()).bearer_auth(access.secret()).send().await?.error_for_status()?;
    // shortcut: plain JSON only; a provider set to sign userinfo responses (application/jwt) fails here.
    let info: serde_json::Value = serde_json::from_slice(&res.bytes().await?)?;
    anyhow::ensure!(info.get("sub").and_then(|v| v.as_str()) == Some(subject), "userinfo is about another subject");
    Ok(info.get(&st.cfg.owner_claim).and_then(|v| v.as_str()).map(str::to_string))
}

pub async fn find_or_create_user(db: &sqlx::PgPool, issuer: &str, subject: &str, locale: &str) -> sqlx::Result<Uuid> {
    let mut tx = db.begin().await?;
    if let Some(id) = sqlx::query_scalar("SELECT user_id FROM identities WHERE issuer = $1 AND subject = $2")
        .bind(issuer)
        .bind(subject)
        .fetch_optional(&mut *tx)
        .await?
    {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, locale) VALUES ($1, $2)").bind(id).bind(locale).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO identities (issuer, subject, user_id) VALUES ($1, $2, $3)")
        .bind(issuer)
        .bind(subject)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

/// Admin routes need a signed-in owner. GETs start sign-in, anything else gets 401.
pub async fn require_owner(State(st): State<AppState>, session: Session, mut req: Request, next: Next) -> Response {
    let exists = match current_owner(&session).await {
        Some(id) => sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM users WHERE id = $1)")
            .bind(id)
            .fetch_one(&st.db)
            .await
            .unwrap_or(false)
            .then_some(id),
        None => None,
    };
    match exists {
        Some(id) => {
            req.extensions_mut().insert(OwnerId(id));
            next.run(req).await
        }
        None if req.method() == Method::GET => {
            let ret = req.uri().path_and_query().map_or("/", |p| p.as_str()).to_string();
            start_login(&st, &session, Host::Admin, &ret).await
        }
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

#[derive(Clone, Copy)]
pub struct OwnerId(pub Uuid);

/// Refuses state-changing admin requests whose `Origin` is not the admin origin.
pub async fn check_origin(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    let origin_ok =
        req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()) == Some(st.cfg.admin_origin.as_str());
    if safe || origin_ok { next.run(req).await } else { StatusCode::FORBIDDEN.into_response() }
}

#[cfg(test)]
mod tests {
    use super::safe_return;

    #[test]
    fn return_urls() {
        assert_eq!(safe_return("/notes/2026/?a=1"), "/notes/2026/?a=1");
        for bad in ["https://evil.example/", "//evil.example/", "/\\evil.example", "evil", ""] {
            assert_eq!(safe_return(bad), "/", "{bad}");
        }
    }
}
