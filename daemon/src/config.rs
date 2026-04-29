use std::{ffi::OsString, str::FromStr, time::Duration};

use snafu::{ResultExt, Snafu};

#[derive(Clone, Debug)]
pub struct Config {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
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
    /// Command to run
    pub command: Vec<OsString>,
}

#[derive(Debug, Snafu)]
pub enum ParseConfigError {
    #[snafu()]
    PicoArgs { source: pico_args::Error },
    #[snafu(display("Missing `--` separating flags from command"))]
    NoDashDash,
    #[snafu()]
    InvalidNumber { source: <f64 as FromStr>::Err },
    #[snafu(display("No command specified after `--`"))]
    EmptyCommand,
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
        let mut args: Vec<_> = std::env::args_os().collect();
        // remove executable path
        args.remove(0);

        // find `--` separating flags from command
        let command = if let Some(dash_dash_idx) = args.iter().position(|arg| arg == "--") {
            let args_after: Vec<_> = args.drain(dash_dash_idx + 1..).collect();
            if args_after.is_empty() {
                return Err(ParseConfigError::EmptyCommand);
            }
            // remove `--`
            args.pop();
            args_after
        } else {
            return Err(ParseConfigError::NoDashDash);
        };

        let mut pargs = pico_args::Arguments::from_vec(args);

        Ok(Config {
            access_key_id: pargs.value_from_str("--access-key-id")?,
            secret_access_key: pargs.value_from_str("--secret-access-key")?,
            session_token: pargs.opt_value_from_str("--session-token")?,
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
            command,
        })
    }
}

pub fn usage() {
    let argv0_os = std::env::args_os()
        .next()
        .unwrap_or_else(|| OsString::from_str("dls3").unwrap());
    eprint!(
        concat!(
            "usage: {} <flags> -- <command>\n",
            "\n",
            "required flags (all strings):\n",
            "  --access-key-id, --secret-access-key, --endpoint-url, --region, --bucket: S3 connection details\n",
            "  --hook-path: path to the library we should LD_PRELOAD into the command\n",
            "  --mountpoint: where files should appear to the command. the daemon must be able to create this location as a symlink.\n",
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
