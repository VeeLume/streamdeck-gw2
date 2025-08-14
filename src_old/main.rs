// src/main.rs
use std::process::ExitCode;
use std::sync::Arc;
use std::{ env, fmt };

mod plugin; // you'll implement plugin::run(args, logger)
mod logger; // must expose { ActionLog, FileLogger }
mod infra; // contains bindings and other infrastructure code
mod core; // contains core event handling and app logic
mod app; // contains app state and action management

use logger::{ ActionLog, FileLogger };

/// Values passed by Stream Deck on launch.
#[derive(Clone, Debug)]
pub struct LaunchArgs {
    pub port: u16,
    pub plugin_uuid: String,
    pub register_event: String,
}

#[derive(Debug)]
enum MainError {
    MissingPort,
    MissingPluginUUID,
    MissingRegisterEvent,
    InvalidPort(String),
    PluginError(anyhow::Error),
}

impl fmt::Display for MainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MainError::MissingPort => write!(f, "missing -port"),
            MainError::MissingPluginUUID => write!(f, "missing -pluginUUID"),
            MainError::MissingRegisterEvent => write!(f, "missing -registerEvent"),
            MainError::InvalidPort(v) => write!(f, "invalid port '{}'", v),
            MainError::PluginError(e) => write!(f, "plugin error: {e}"),
        }
    }
}

fn main() -> ExitCode {
    // Bring up file logger first so everything after this is captured.
    let logger = match FileLogger::from_appdata() {
        Ok(l) => Arc::new(l),
        Err(e) => {
            eprintln!("Failed to initialize logger: {e}");
            return ExitCode::from(1);
        }
    };

    // Redirect println!/eprintln! into the same file.
    if let Err(e) = logger.redirect_stdout_stderr() {
        // Last resort if redirection fails
        log!(logger, "failed to redirect stdout/stderr: {}", e);
    }

    // Convert panics into log lines instead of noisy stderr dumps.
    {
        std::panic::set_hook({
            let logger = Arc::clone(&logger);
            Box::new(move |info| {
                let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                    *s
                } else if let Some(s) = info.payload().downcast_ref::<String>() {
                    s.as_str()
                } else {
                    "unknown panic"
                };
                log!(logger, "💥 panic: {} at {:?}", msg, info.location());
            })
        });
    }

    match run(logger.clone()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log!(logger, "fatal: {err}");
            // Map to stable exit codes similar to your old main.
            match err {
                MainError::MissingPort => ExitCode::from(2),
                MainError::MissingPluginUUID => ExitCode::from(3),
                MainError::MissingRegisterEvent => ExitCode::from(4),
                MainError::InvalidPort(_) | MainError::PluginError(_) => ExitCode::from(5),
            }
        }
    }
}

fn run(logger: Arc<dyn ActionLog>) -> Result<(), MainError> {
    let args = parse_launch_args().map_err(|e| {
        eprintln!("Usage: plugin -port <PORT> -pluginUUID <UUID> -registerEvent <JSON>");
        e
    })?;

    let url = format!("ws://127.0.0.1:{}", args.port);
    log!(
        logger,
        "🔌 connecting to {url}  uuid={}  registerEvent={}",
        args.plugin_uuid,
        args.register_event
    );

    // hand off to the real application (new world)
    plugin::run(args, logger).map_err(MainError::PluginError)
}

fn parse_launch_args() -> Result<LaunchArgs, MainError> {
    let argv: Vec<String> = env::args().collect();

    let port_str = value_after(&argv, "-port").ok_or(MainError::MissingPort)?;
    let plugin_uuid = value_after(&argv, "-pluginUUID").ok_or(MainError::MissingPluginUUID)?;
    let register_event = value_after(&argv, "-registerEvent").ok_or(
        MainError::MissingRegisterEvent
    )?;

    let port = port_str.parse::<u16>().map_err(|_| MainError::InvalidPort(port_str.to_string()))?;

    Ok(LaunchArgs {
        port,
        plugin_uuid: plugin_uuid.to_string(),
        register_event: register_event.to_string(),
    })
}

fn value_after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter()
        .position(|a| a == flag)
        .and_then(|i| argv.get(i + 1))
        .map(|s| s.as_str())
}
