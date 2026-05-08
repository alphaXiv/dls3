//! Utilities for working with S3

use s3::{Bucket, Region, creds::Credentials};

use crate::config::Config;

pub fn create_bucket(config: &Config) -> Box<Bucket> {
    Bucket::new(
        &config.bucket,
        Region::Custom {
            region: config.region.clone(),
            endpoint: config.endpoint_url.clone(),
        },
        Credentials::new(
            Some(&config.auth.access_key_id),
            Some(&config.auth.secret_access_key),
            None,
            config.auth.session_token.as_deref(),
            None,
        )
        .unwrap(),
    )
    .unwrap()
}
