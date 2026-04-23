use std::borrow::Cow;

#[derive(Clone, Debug)]
pub struct Config {
    pub access_key_id: Cow<'static, str>,
    pub secret_access_key: Cow<'static, str>,
    pub session_token: Option<Cow<'static, str>>,
    pub endpoint_url: Cow<'static, str>,
    pub region: Cow<'static, str>,
    pub bucket: Cow<'static, str>,
    pub prefix: Cow<'static, str>,
    pub backing_path: Cow<'static, str>,
    pub socket_path: Cow<'static, str>,
}

/// Hardcoded config during development
pub const CONFIG: Config = Config {
    access_key_id: Cow::Borrowed("GK_ACCESS"),
    secret_access_key: Cow::Borrowed("GK_SECRETSECRETSECRET"),
    session_token: None,
    endpoint_url: Cow::Borrowed("http://localhost:3900"),
    region: Cow::Borrowed("garage"),
    bucket: Cow::Borrowed("garage"),
    prefix: Cow::Borrowed(""),
    backing_path: Cow::Borrowed("/root/dls3-store"),
    socket_path: Cow::Borrowed("/root/dls3.sock"),
};
