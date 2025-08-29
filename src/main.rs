use crate::gw2::bindings_adapter::Gw2BindingsAdapter;
use crate::gw2::exec_adapter::Gw2ExecAdapter;
use crate::gw2::gw2_api_adapter::Gw2ApiAdapter;
use crate::gw2::mumble_adapter::MumbleAdapter;
use crate::gw2::shared::SharedBindings;
use crate::gw2::shared::{ActiveChar, TemplateStore};
use constcat::concat;
use std::{process::exit, sync::Arc};
use streamdeck_lib::prelude::*;

mod actions {
    pub mod set_template;
    pub mod settings;
}
mod gw2 {
    pub mod bindings_adapter;
    pub mod binds;
    pub mod enums;
    pub mod exec_adapter;
    pub mod gw2_api_adapter;
    pub mod mumble;
    pub mod mumble_adapter;
    pub mod shared;
}
mod topics;

const PLUGIN_ID: &str = "icu.veelume.gw2";

fn main() {
    let logger: Arc<dyn ActionLog> = match FileLogger::from_appdata(PLUGIN_ID) {
        Ok(logger) => Arc::new(logger),
        Err(e) => {
            eprintln!("Failed to create logger: {e}");
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

    let hooks = AppHooks::default().append(|cx, ev| {
        if let HookEvent::DidReceiveGlobalSettings(settings) = ev {
            info!(cx.log(), "Received global settings: {:?}", settings);
            if let Some(shared_binds) = cx.try_ext::<SharedBindings>() {
                if let Err(e) = shared_binds.replace_from_globals(settings) {
                    error!(cx.log(), "Failed to replace bindings from globals: {}", e);
                } else {
                    info!(cx.log(), "Bindings updated from globals.");
                }
            }
        }
    });

    let shared_binds = SharedBindings::default();
    let template_store = TemplateStore::default();
    let active_char = ActiveChar::default();

    let plugin = match PluginBuilder::new()
        .set_hooks(hooks)
        .add_adapter(Gw2BindingsAdapter::new())
        .add_adapter(Gw2ApiAdapter::new())
        .add_adapter(MumbleAdapter::new())
        .add_adapter(Gw2ExecAdapter::new())
        .add_extension(Arc::new(shared_binds))
        .add_extension(Arc::new(template_store))
        .add_extension(Arc::new(active_char))
        .add_action(ActionFactory::new(
            concat!(PLUGIN_ID, ".set-template"),
            actions::set_template::SetTemplateAction::default,
        ))
        .add_action(ActionFactory::new(
            concat!(PLUGIN_ID, ".settings"),
            actions::settings::SettingsAction::default,
        ))
        .build()
    {
        Ok(plugin) => plugin,
        Err(e) => {
            error!(logger, "Failed to build plugin: {}", e);
            exit(3);
        }
    };

    let cfg = RunConfig::default().set_log_websocket(false);

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
