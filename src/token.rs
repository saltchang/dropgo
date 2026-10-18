//! Signed values: password unlock cookies and path access tokens.
//! Both are `<expires unix>.<base64url HMAC>`, bound to a share and its current credential.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;

pub const UNLOCK_SECS: i64 = 7 * 24 * 3600;
pub const PATH_TOKEN_SECS: i64 = 12 * 3600;

#[derive(Clone, Copy)]
pub enum Kind {
    Unlock,
    Path,
}

fn mac(secret: &str, kind: Kind, share: Uuid, credential: &str, exp: i64) -> Hmac<Sha256> {
    let mut m = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("any key length");
    let kind = match kind {
        Kind::Unlock => "unlock",
        Kind::Path => "path",
    };
    // NUL separators: no field can contain one, so fields cannot shift into each other.
    for part in [kind, &share.to_string(), credential, &exp.to_string()] {
        m.update(part.as_bytes());
        m.update(b"\0");
    }
    m
}

pub fn sign(secret: &str, kind: Kind, share: Uuid, credential: &str, now: i64) -> String {
    let exp = now
        + match kind {
            Kind::Unlock => UNLOCK_SECS,
            Kind::Path => PATH_TOKEN_SECS,
        };
    let sig = mac(secret, kind, share, credential, exp).finalize().into_bytes();
    format!("{exp}.{}", URL_SAFE_NO_PAD.encode(sig))
}

pub fn verify(secret: &str, kind: Kind, share: Uuid, credential: &str, token: &str, now: i64) -> bool {
    let Some((exp, sig)) = token.split_once('.') else { return false };
    let (Ok(exp), Ok(sig)) = (exp.parse::<i64>(), URL_SAFE_NO_PAD.decode(sig)) else { return false };
    exp > now && mac(secret, kind, share, credential, exp).verify_slice(&sig).is_ok()
}

/// What a token is bound to: the password hash, or `owner` for owner-only shares.
pub fn credential(visibility: &str, password_hash: Option<&str>) -> String {
    match visibility {
        "password" => password_hash.unwrap_or_default().to_string(),
        v => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let t = sign("k", Kind::Path, a, "h1", 1000);
        assert!(verify("k", Kind::Path, a, "h1", &t, 1000));
        assert!(!verify("k", Kind::Path, b, "h1", &t, 1000), "other share");
        assert!(!verify("k", Kind::Path, a, "h2", &t, 1000), "password changed");
        assert!(!verify("k", Kind::Unlock, a, "h1", &t, 1000), "other kind");
        assert!(!verify("other", Kind::Path, a, "h1", &t, 1000), "other secret");
        assert!(!verify("k", Kind::Path, a, "h1", &t, 1000 + PATH_TOKEN_SECS), "expired");
        assert!(!verify("k", Kind::Path, a, "h1", "garbage", 1000));
        let forged = format!("{}.{}", 1000 + 10 * PATH_TOKEN_SECS, t.split_once('.').unwrap().1);
        assert!(!verify("k", Kind::Path, a, "h1", &forged, 1000), "moved expiry");
    }
}
