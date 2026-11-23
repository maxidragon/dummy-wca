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
//! A code asked for with PKCE also carries its challenge:
//! `code-<WCA ID>-<scopes>-<method>-<challenge>`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use sha2::{Digest, Sha256};

/// The WCA staging seed's confidential client, which must send its secret.
pub const CONFIDENTIAL_CLIENT_ID: &str = "example-application-id";
/// The WCA staging seed's public client (for apps that can't keep a secret,
/// like TNoodle), which may leave the secret out and use PKCE instead.
pub const PUBLIC_CLIENT_ID: &str = "example-application-id-public";
/// Both clients' secret, as in the WCA staging seed.
pub const CLIENT_SECRET: &str = "example-secret";

/// Doorkeeper's default `pkce_code_challenge_methods`, which the WCA keeps.
pub const CODE_CHALLENGE_METHODS: [&str; 2] = ["plain", "S256"];

/// Whether the client exists, for the authorize page.
pub fn is_client(client_id: &str) -> bool {
    client_id == CONFIDENTIAL_CLIENT_ID || client_id == PUBLIC_CLIENT_ID
}

/// Doorkeeper's `by_uid_and_secret`: a public client may leave its secret
/// out, but a secret that is sent must be right.
pub fn client_authenticates(client_id: &str, secret: Option<&str>) -> bool {
    let secret = secret.filter(|secret| !secret.is_empty());
    match client_id {
        CONFIDENTIAL_CLIENT_ID => secret == Some(CLIENT_SECRET),
        PUBLIC_CLIENT_ID => secret.is_none_or(|secret| secret == CLIENT_SECRET),
        _ => false,
    }
}

/// A PKCE code challenge (RFC 7636).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Challenge<'a> {
    /// One of [`CODE_CHALLENGE_METHODS`].
    pub method: &'a str,
    pub value: &'a str,
}

/// What an authorization code grants.
#[derive(Debug, PartialEq, Eq)]
pub struct CodeGrant<'a> {
    pub wca_id: &'a str,
    pub scopes: Vec<&'a str>,
    pub challenge: Option<Challenge<'a>>,
}

impl CodeGrant<'_> {
    /// Doorkeeper's `validate_code_verifier`: a verifier must answer the
    /// challenge, so one sent for a code without a challenge fails too.
    pub fn verifier_matches(&self, verifier: &str) -> bool {
        match self.challenge {
            Some(Challenge {
                method: "S256",
                value,
            }) => value == s256(verifier),
            Some(Challenge {
                method: "plain",
                value,
            }) => value == verifier,
            _ => false,
        }
    }
}

/// Doorkeeper's `generate_code_challenge`: base64url of the verifier's
/// SHA-256, without padding.
fn s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// What the authorize page appends to `code-<WCA ID>`.
pub fn code_suffix(scopes: &[&str], challenge: Option<Challenge>) -> String {
    let mut suffix = format!("-{}", scopes.join(","));
    if let Some(challenge) = challenge {
        suffix.push_str(&format!("-{}-{}", challenge.method, challenge.value));
    }
    suffix
}

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
    scopes: Option<Vec<&'a str>>,
}

impl Caller<'_> {
    /// Whether the token was granted this scope.
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes
            .as_ref()
            .is_none_or(|scopes| scopes.contains(&scope))
    }
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

/// What an authorization code grants, or `None` if it is malformed.
/// Without scopes, it grants every scope.
pub fn code_grant(code: &str) -> Option<CodeGrant<'_>> {
    let (_, rest) = code.split_once('-')?;
    // The challenge comes last, as it may hold dashes itself.
    let mut parts = rest.splitn(4, '-');
    let wca_id = parts.next().filter(|wca_id| !wca_id.is_empty())?;
    let scopes = match parts.next() {
        Some(scopes) => parse_scopes(scopes)?,
        None => SCOPES.to_vec(),
    };
    let challenge = match (parts.next(), parts.next()) {
        (None, _) => None,
        (Some(method), Some(value))
            if CODE_CHALLENGE_METHODS.contains(&method) && !value.is_empty() =>
        {
            Some(Challenge { method, value })
        }
        _ => return None,
    };
    Some(CodeGrant {
        wca_id,
        scopes,
        challenge,
    })
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

/// A refresh token's WCA ID and scopes, without its nonce.
pub fn refresh_grant(token: &str) -> Option<(&str, Vec<&str>)> {
    let (without_nonce, nonce) = token.rsplit_once('-')?;
    nonce.parse::<u64>().ok()?;
    match code_grant(without_nonce)? {
        CodeGrant {
            wca_id,
            scopes,
            challenge: None,
        } => Some((wca_id, scopes)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_tokens_never_expire_and_may_do_anything() {
        let caller = caller_at("token-2012VAQR01", i64::MAX).unwrap();
        assert_eq!(caller.wca_id, "2012VAQR01");
        assert!(caller.has_scope("manage_registrations"));
    }

    #[test]
    fn issued_tokens_expire_and_keep_their_scopes() {
        let token = access_token("2012VAQR01", &["public", "manage_registrations"], 100);
        let caller = caller_at(&token, 99).unwrap();
        assert_eq!(caller.wca_id, "2012VAQR01");
        assert!(caller.has_scope("manage_registrations"));
        assert!(!caller.has_scope("manage_competitions"));
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
        let grant = code_grant("code-2012VAQR01").unwrap();
        assert_eq!(grant.wca_id, "2012VAQR01");
        assert_eq!(grant.scopes, SCOPES.to_vec());
        assert_eq!(grant.challenge, None);
        let grant = code_grant("code-2012VAQR01-public,manage_registrations").unwrap();
        assert_eq!(grant.scopes, ["public", "manage_registrations"]);
        assert_eq!(code_grant("code-2012VAQR01-everything"), None);
    }

    // S256_CHALLENGE is base64url(SHA-256(VERIFIER)), worked out outside Rust.
    const VERIFIER: &str = "dBjftJeZ4CVP-mJ92K1qW-g7wEnzyW2eGO40wjPsUEo";
    const S256_CHALLENGE: &str = "qzQb9tSSnS0U71xfMidDXx1f5DWgBrv-BVcnFnWJC8E";

    #[test]
    fn codes_carry_their_challenge_dashes_and_all() {
        let challenge = Challenge {
            method: "S256",
            value: S256_CHALLENGE,
        };
        let code = format!(
            "code-2012VAQR01{}",
            code_suffix(&["public"], Some(challenge))
        );
        let grant = code_grant(&code).unwrap();
        assert_eq!(grant.scopes, ["public"]);
        assert_eq!(grant.challenge, Some(challenge));
        assert_eq!(code_grant("code-2012VAQR01-public-sha512-abc"), None);
        assert_eq!(code_grant("code-2012VAQR01-public-plain-"), None);
        assert_eq!(code_grant("code-2012VAQR01-public-plain"), None);
    }

    #[test]
    fn verifiers_must_answer_the_challenge_like_doorkeeper() {
        let code = format!("code-2012VAQR01-public-S256-{}", S256_CHALLENGE);
        let s256 = code_grant(&code).unwrap();
        assert!(s256.verifier_matches(VERIFIER));
        assert!(!s256.verifier_matches(S256_CHALLENGE));
        let code = format!("code-2012VAQR01-public-plain-{}", VERIFIER);
        let plain = code_grant(&code).unwrap();
        assert!(plain.verifier_matches(VERIFIER));
        assert!(!plain.verifier_matches("something-else"));
        let without = code_grant("code-2012VAQR01-public").unwrap();
        assert!(!without.verifier_matches(VERIFIER));
    }

    #[test]
    fn only_the_public_client_may_leave_its_secret_out() {
        assert!(client_authenticates(
            CONFIDENTIAL_CLIENT_ID,
            Some(CLIENT_SECRET)
        ));
        assert!(!client_authenticates(CONFIDENTIAL_CLIENT_ID, None));
        assert!(!client_authenticates(CONFIDENTIAL_CLIENT_ID, Some("")));
        assert!(client_authenticates(PUBLIC_CLIENT_ID, None));
        assert!(client_authenticates(PUBLIC_CLIENT_ID, Some("")));
        assert!(client_authenticates(PUBLIC_CLIENT_ID, Some(CLIENT_SECRET)));
        assert!(!client_authenticates(PUBLIC_CLIENT_ID, Some("wrong")));
        assert!(!client_authenticates("someone-else", Some(CLIENT_SECRET)));
    }

    #[test]
    fn refresh_tokens_carry_their_grant() {
        let token = refresh_token("2012VAQR01", &["public"], 7);
        assert_eq!(refresh_grant(&token), Some(("2012VAQR01", vec!["public"])));
        assert_eq!(refresh_grant("refresh-2012VAQR01-public"), None);
        assert_eq!(refresh_grant("refresh-2012VAQR01-public-plain-abc-7"), None);
    }
}
