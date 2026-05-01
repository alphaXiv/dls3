use std::{ffi::OsString, str::FromStr, time::Duration};

use snafu::{ResultExt, Snafu};

#[derive(Clone, Debug)]
pub struct AwsAuth {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub auth: AwsAuth,
    pub endpoint_url: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    /// How frequently to scan the entire bucket for new/deleted files.
    pub refetch_all_interval: Duration,
    /// How frequently to check for updated versions of opened files.
    pub refetch_open_interval: Duration,
    /// Path to the library to LD_PRELOAD
    pub hook_path: String,
    pub mountpoint: String,
    /// Path where we will write environment variables for the victim
    pub env_destination: String,
}

#[derive(Debug, Snafu)]
pub enum ParseConfigError {
    #[snafu()]
    PicoArgs { source: pico_args::Error },
    #[snafu()]
    InvalidNumber { source: <f64 as FromStr>::Err },
}

impl From<pico_args::Error> for ParseConfigError {
    fn from(value: pico_args::Error) -> Self {
        Self::PicoArgs { source: value }
    }
}

fn parse_duration(string: &str) -> Result<Duration, ParseConfigError> {
    let seconds: f64 = string.parse().context(InvalidNumberSnafu)?;
    Ok(Duration::from_secs_f64(seconds))
}

impl Config {
    pub fn parse_args() -> Result<Config, ParseConfigError> {
        let mut pargs = pico_args::Arguments::from_env();

        Ok(Config {
            auth: AwsAuth {
                access_key_id: pargs.value_from_str("--access-key-id")?,
                secret_access_key: pargs.value_from_str("--secret-access-key")?,
                session_token: pargs.opt_value_from_str("--session-token")?,
            },
            endpoint_url: pargs.value_from_str("--endpoint-url")?,
            region: pargs.value_from_str("--region")?,
            bucket: pargs.value_from_str("--bucket")?,
            prefix: pargs
                .opt_value_from_str("--prefix")?
                .unwrap_or(String::new()),
            refetch_all_interval: pargs
                .opt_value_from_fn("--refetch-all-interval", parse_duration)?
                .unwrap_or(Duration::from_secs(30)),
            refetch_open_interval: pargs
                .opt_value_from_fn("--refetch-open-interval", parse_duration)?
                .unwrap_or(Duration::from_secs(5)),
            hook_path: pargs.value_from_str("--hook-path")?,
            mountpoint: pargs.value_from_str("--mountpoint")?,
            env_destination: pargs.value_from_str("--env-destination")?,
        })
    }
}

pub fn usage() {
    let argv0_os = std::env::args_os()
        .next()
        .unwrap_or_else(|| OsString::from_str("dls3").unwrap());
    eprint!(
        concat!(
            "usage: {} <flags>\n",
            "\n",
            "required flags (all strings):\n",
            "  --access-key-id, --secret-access-key, --endpoint-url, --region, --bucket: S3 connection details\n",
            "  --hook-path: path to the library we should LD_PRELOAD into the command\n",
            "  --mountpoint: where files should appear to the command. the daemon must be able to create this location as a symlink.\n",
            "  --env-destination: where to write environment variables for victims to use",
            "\n",
            "optional flags:\n",
            "  --session-token <string>: if required by your S3 authentication\n",
            "  --prefix <string>: only map objects matching this prefix\n",
            "    default: no prefix (entire bucket contents)\n",
            "  --refetch-all-interval <seconds>: how often to check for new/deleted objects in the bucket\n",
            "    default: 30\n",
            "  --refetch-open-interval <seconds>: how often to check for changes to currently open objects in the bucket\n",
            "    default: 5\n",
        ),
        argv0_os.to_string_lossy()
    );
}
