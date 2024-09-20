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