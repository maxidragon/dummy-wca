//! Who a bearer token belongs to, and what it may do.
//!
//! Tokens carry everything in themselves, so they survive a restart:
//!
//! - `token-<WCA ID>`: the fixed token of every account. It never expires
//!   and has every scope, so tests can use it without signing in.
//! - `token-<WCA ID>-<expires at, Unix seconds>-<scopes, comma-separated>`:
//!   what `POST /oauth/token` hands out. It expires like a WCA token and has
//!   only the scopes it was granted.
//!
//! Codes work the same way: `code-<WCA ID>` grants every scope, and
//! `code-<WCA ID>-<scopes>` (what the authorize page sends back) only those.

use chrono::Utc;

/// The scopes the WCA's OAuth server knows: `public` by default, the rest
/// on request.
pub const SCOPES: [&str; 9] = [
    "public",
    "dob",
    "email",
    "manage_competitions",
    "manage_registrations",
    "openid",
    "profile",
    "cms",
    "translations",
];

pub const DEFAULT_SCOPE: &str = "public";

/// The signed-in user behind a token.
#[derive(Debug, PartialEq, Eq)]
pub struct Caller<'a> {
    pub wca_id: &'a str,
    /// `None` for the fixed `token-<WCA ID>` tokens, which may do anything.
    pub scopes: Option<Vec<&'a str>>,
}

/// The caller behind a bearer token, or `None` if it is malformed or expired.
pub fn caller(token: &str) -> Option<Caller<'_>> {
    caller_at(token, Utc::now().timestamp())
}

fn caller_at(token: &str, now: i64) -> Option<Caller<'_>> {
    let (_, rest) = token.split_once('-')?;
    let mut parts = rest.splitn(3, '-');
    let wca_id = parts.next().filter(|wca_id| !wca_id.is_empty())?;
    let Some(expires_at) = parts.next() else {
        return Some(Caller {
            wca_id,
            scopes: None,
        });
    };
    let expires_at: i64 = expires_at.parse().ok()?;
    if now >= expires_at {
        return None;
    }
    Some(Caller {
        wca_id,
        scopes: Some(parts.next().unwrap_or_default().split(',').collect()),
    })
}

/// The WCA ID and scopes an authorization code or refresh token grants.
/// Both look like `<kind>-<WCA ID>[-<scopes>]`; without scopes, every scope.
pub fn grant(code: &str) -> Option<(&str, Vec<&str>)> {
    let (_, rest) = code.split_once('-')?;
    let (wca_id, scopes) = match rest.split_once('-') {
        Some((wca_id, scopes)) => (wca_id, parse_scopes(scopes)?),
        None => (rest, SCOPES.to_vec()),
    };
    (!wca_id.is_empty()).then_some((wca_id, scopes))
}

/// Scopes separated by commas or spaces. Unknown scopes are refused, as the
/// WCA does; none at all means the default scope.
pub fn parse_scopes(scopes: &str) -> Option<Vec<&'static str>> {
    let mut parsed: Vec<&'static str> = Vec::new();
    for scope in scopes
        .split([',', ' ', '+'])
        .filter(|scope| !scope.is_empty())
    {
        let known = SCOPES.iter().find(|known| **known == scope)?;
        if !parsed.contains(known) {
            parsed.push(known);
        }
    }
    if parsed.is_empty() {
        parsed.push(DEFAULT_SCOPE);
    }
    Some(parsed)
}

pub fn access_token(wca_id: &str, scopes: &[&str], expires_at: i64) -> String {
    format!("token-{}-{}-{}", wca_id, expires_at, scopes.join(","))
}

/// `nonce` keeps every refresh token distinct, so each can be used once.
pub fn refresh_token(wca_id: &str, scopes: &[&str], nonce: u64) -> String {
    format!("refresh-{}-{}-{}", wca_id, scopes.join(","), nonce)
}

/// A refresh token's grant, without its nonce.
pub fn refresh_grant(token: &str) -> Option<(&str, Vec<&str>)> {
    let (without_nonce, nonce) = token.rsplit_once('-')?;
    nonce.parse::<u64>().ok()?;
    grant(without_nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_tokens_never_expire_and_may_do_anything() {
        let caller = caller_at("token-2012VAQR01", i64::MAX).unwrap();
        assert_eq!(caller.wca_id, "2012VAQR01");
        assert_eq!(caller.scopes, None);
    }

    #[test]
    fn issued_tokens_expire_and_keep_their_scopes() {
        let token = access_token("2012VAQR01", &["public", "manage_registrations"], 100);
        let caller = caller_at(&token, 99).unwrap();
        assert_eq!(caller.wca_id, "2012VAQR01");
        assert_eq!(caller.scopes, Some(vec!["public", "manage_registrations"]));
        assert_eq!(caller_at(&token, 100), None);
    }

    #[test]
    fn malformed_tokens_are_refused() {
        assert_eq!(caller_at("token", 0), None);
        assert_eq!(caller_at("token-", 0), None);
        assert_eq!(caller_at("token-2012VAQR01-soon-public", 0), None);
    }

    #[test]
    fn codes_grant_their_scopes_or_all() {
        let (wca_id, scopes) = grant("code-2012VAQR01").unwrap();
        assert_eq!(wca_id, "2012VAQR01");
        assert_eq!(scopes, SCOPES.to_vec());
        let (_, scopes) = grant("code-2012VAQR01-public,manage_registrations").unwrap();
        assert_eq!(scopes, ["public", "manage_registrations"]);
        assert_eq!(grant("code-2012VAQR01-everything"), None);
    }

    #[test]
    fn refresh_tokens_carry_their_grant() {
        let token = refresh_token("2012VAQR01", &["public"], 7);
        assert_eq!(refresh_grant(&token), Some(("2012VAQR01", vec!["public"])));
        assert_eq!(refresh_grant("refresh-2012VAQR01-public"), None);
    }
}
