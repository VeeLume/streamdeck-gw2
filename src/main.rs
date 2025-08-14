use std::{ process::exit, sync::Arc };
use constcat::concat;
use streamdeck_lib::hooks::AppHooks;
use streamdeck_lib::{
    bootstrap::{ parse_launch_args, RunConfig },
    logger::{ ActionLog, FileLogger },
    prelude::{ ActionFactory, PluginBuilder },
    runtime::run,
    error,
    info,
};

use crate::gw2::bindings_adapter::Gw2BindingsAdapter;
use crate::gw2::{ shared::SharedBindings };

mod actions;
mod gw2 {
    pub mod enums;
    pub mod binds;
    pub mod shared;
    pub mod bindings_adapter;
}

const PLUGIN_ID: &str = "icu.veelume.gw2";

fn main() {
    let logger: Arc<dyn ActionLog> = match FileLogger::from_appdata(PLUGIN_ID) {
        Ok(logger) => Arc::new(logger),
        Err(e) => {
            eprintln!("Failed to create logger: {}", e);
            exit(1);
        }
    };

    let args = match parse_launch_args() {
        Ok(args) => args,
        Err(e) => {
            error!(logger, "Failed to parse launch arguments: {}", e);
            exit(2);
        }
    };

    let hooks = AppHooks::default()
        .on_init(|cx| {
            info!(cx.log(), "Plugin initialized with ID: {}", PLUGIN_ID);
            // Reset globals to default state
            // cx.globals().reset(
            //     true,
            //     cx.sd()
            // );
            // thread::sleep(std::time::Duration::from_secs(3));
        })
        .on_did_receive_global_settings(|cx, settings| {
            if let Some(shared_binds) = cx.try_ext::<SharedBindings>() {
                if let Err(e) = shared_binds.replace_from_globals(settings) {
                    error!(cx.log(), "Failed to replace bindings from globals: {}", e);
                } else {
                    info!(cx.log(), "Bindings updated from globals.");
                }
            }
        })
        .on_application_did_launch(|cx, ev| {
            info!(cx.log(), "Application launched: {:?}", ev);
        })
        .on_application_did_terminate(|cx, ev| {
            info!(cx.log(), "Application terminated: {:?}", ev);
        })
        .on_action_notify(|cx, cx_id, topic, data: &Option<serde_json::Value>| {
            info!(
                cx.log(),
                "Action notification received: cx_id={:?}, topic={}, data={:?}",
                cx_id,
                topic,
                data
            );
        });

    let shared_binds = SharedBindings::default();

    let plugin = match
        PluginBuilder::new()
            .hooks(hooks)
            .add_adapter(Gw2BindingsAdapter::new())
            .with_extension(Arc::new(shared_binds))
            .register_action(
                ActionFactory::new(concat!(PLUGIN_ID, ".set-template"), ||
                    actions::SetTemplateAction::default()
                )
            )
            .register_action(
                ActionFactory::new(concat!(PLUGIN_ID, ".settings"), ||
                    actions::SettingsAction::default()
                )
            )
            .build()
    {
        Ok(plugin) => plugin,
        Err(e) => {
            error!(logger, "Failed to build plugin: {}", e);
            exit(3);
        }
    };

    let cfg = RunConfig::default().log_incoming_websocket(false);

    match run(plugin, args, logger.clone(), cfg) {
        Ok(_) => {
            info!(logger, "Plugin exited successfully.");
        }
        Err(e) => {
            error!(logger, "Plugin run failed: {}", e);
            exit(4);
        }
    }
}
