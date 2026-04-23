//! Utilities for working with S3

use aws_sdk_s3::{
    Client,
    config::{Credentials, Region},
};

use crate::config::Config;

pub fn create_client(config: &Config) -> Client {
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::new(
            config.access_key_id.as_ref(),
            config.secret_access_key.as_ref(),
            config.session_token.as_ref().map(|t| t.to_string()),
            None,
            "dls3",
        ))
        .endpoint_url(config.endpoint_url.as_ref())
        .region(Region::new(config.region.clone()))
        .force_path_style(true)
        .build();
    Client::from_conf(conf)
}
