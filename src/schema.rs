use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct OauthRequestOptions {
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub response_type: String,
    pub client_id: String,
    pub state: Option<String>,
    /// PKCE (RFC 7636): the challenge the token request's `code_verifier`
    /// must answer.
    pub code_challenge: Option<String>,
    /// `plain` or `S256`; required with a `code_challenge`.
    pub code_challenge_method: Option<String>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct OauthTokenRequestOptions {
    /// For `grant_type=authorization_code`.
    pub code: Option<String>,
    /// For `grant_type=refresh_token`.
    pub refresh_token: Option<String>,
    pub client_id: String,
    /// Optional for the public client, which has no secret to keep.
    pub client_secret: Option<String>,
    pub redirect_uri: Option<String>,
    pub grant_type: String,
    /// PKCE (RFC 7636): answers the code's `code_challenge`.
    pub code_verifier: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct SearchUsersOptions {
    pub q: String,
}
/// `GET /api/v0/competitions?managed_by_me=true&start=...&end=...&sort=start_date`
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct ManagedCompetitionsOptions {
    pub managed_by_me: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub sort: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct ResetOptions {
    pub id: Option<String>,
}
