use std::process::ExitCode;
use std::{ env };
use std::sync::Arc;

use crate::logger::{ ActionLog, FileLogger };
use crate::plugin::{ run_plugin };
mod logger;
mod plugin;
mod action_handlers;
mod config;
mod bindings;
mod plugin_state;
mod app;

fn main() -> ExitCode {
    let logger = match FileLogger::from_appdata() {
        Ok(logger) => Arc::new(logger),
        Err(e) => {
            eprintln!("Failed to initialize logger: {}", e);
            return ExitCode::from(1);
        }
    };

    if let Err(e) = safe_main(logger.clone()) {
        log!(logger, "Error: {:?}", e);
        match e {
            SafeMainError::MissingPort() => {
                log!(logger, "Error: Missing -port argument");
                return ExitCode::from(2);
            }
            SafeMainError::MissingPluginUUID() => {
                log!(logger, "Error: Missing -pluginUUID argument");
                return ExitCode::from(3);
            }
            SafeMainError::MissingRegisterEvent() => {
                log!(logger, "Error: Missing -registerEvent argument");
                return ExitCode::from(4);
            }
            SafeMainError::PluginError => {
                return ExitCode::from(5);
            }
        }
    }

    ExitCode::SUCCESS
}

#[derive(Debug)]
enum SafeMainError {
    MissingPort(),
    MissingPluginUUID(),
    MissingRegisterEvent(),
    PluginError,
}

fn safe_main(logger: Arc<dyn ActionLog>) -> Result<(), SafeMainError> {
    let args: Vec<String> = env::args().collect();
    let port = args
        .iter()
        .position(|a| a == "-port")
        .and_then(|i| args.get(i + 1))
        .ok_or(SafeMainError::MissingPort())?;

    let uuid = args
        .iter()
        .position(|a| a == "-pluginUUID")
        .and_then(|i| args.get(i + 1))
        .ok_or(SafeMainError::MissingPluginUUID())?;

    let register_event = args
        .iter()
        .position(|a| a == "-registerEvent")
        .and_then(|i| args.get(i + 1))
        .ok_or(SafeMainError::MissingRegisterEvent())?;

    let url = format!("ws://127.0.0.1:{port}");

    log!(
        logger,
        "🔌 Connecting to {} with plugin UUID: {} and register event: {}",
        url,
        uuid,
        register_event
    );

    // Delegate the actual plugin logic to another module
    run_plugin(url, uuid, register_event, logger).map_err(|_| SafeMainError::PluginError)?;

    Ok(())
}
