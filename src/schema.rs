use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct OauthRequestOptions {
    pub redirect_uri: String,
    pub scope: String,
    pub response_type: String,
    pub client_id: String,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct OauthTokenRequestOptions {
    pub code: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub grant_type: String,
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
