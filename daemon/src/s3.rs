//! Utilities for working with S3

use aws_sdk_s3::{
    Client,
    config::{Credentials, Region},
};

use crate::config::Config;

pub fn create_client(config: &Config) -> Client {
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::new(
            &config.auth.access_key_id,
            &config.auth.secret_access_key,
            config.auth.session_token.clone(),
            None,
            "dls3",
        ))
        .endpoint_url(&config.endpoint_url)
        .region(Region::new(config.region.clone()))
        .force_path_style(true)
        .build();
    Client::from_conf(conf)
}
