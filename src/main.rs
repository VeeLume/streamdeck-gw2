use std::{ process::exit, sync::Arc };
use constcat::concat;
use streamdeck_lib::adapters::StartPolicy;
use streamdeck_lib::hooks::AppHooks;
use streamdeck_lib::prelude::Context;
use streamdeck_lib::{
    bootstrap::{ parse_launch_args, RunConfig },
    logger::{ ActionLog, FileLogger },
    prelude::{ ActionFactory, PluginBuilder },
    runtime::run,
    error,
    info,
};

use crate::gw2::bindings_adapter::Gw2BindingsAdapter;
use crate::gw2::exec_adapter::Gw2ExecAdapter;
use crate::gw2::gw2_api_adapter::Gw2ApiAdapter;
use crate::gw2::mumble_adapter::MumbleAdapter;
use crate::gw2::shared::{ ActiveChar, InCombat, TemplateStore };
use crate::gw2::{ shared::SharedBindings };

mod actions {
    pub mod set_template;
    pub mod settings;
}
mod gw2 {
    pub mod enums;
    pub mod binds;
    pub mod shared;
    pub mod bindings_adapter;
    pub mod gw2_api_adapter;
    pub mod mumble_adapter;
    pub mod exec_adapter;
}

mod topics {
    pub const GW2_EXEC_QUEUE: &str = "gw2.exec.queue";
    pub const MUMBLE_ACTIVE: &str = "mumble.active-character";
    pub const GW2_API_FETCHED: &str = "gw2-api.fetched";
    pub const GW2_API_CHAR_CHANGED: &str = "gw2-api.character-changed";
    pub const GW2_API_TPL_CHANGED: &str = "gw2-api.template-changed";
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

    let log_action_notify = |cx: &Context, cx_id: Option<&str>, topic: &str, data: &Option<serde_json::Value>| {
        info!(
            cx.log(),
            "Action notification received: cx_id={:?}, topic={}, data={:?}",
            cx_id,
            topic,
            data
        );
    };

    let log_adapter_notify = |cx: &Context, topic: &str, data: &Option<serde_json::Value>, cx_id: Option<&str>| {
        info!(
            cx.log(),
            "Adapter notification received: topic={}, data={:?}, cx_id={:?}",
            topic,
            data,
            cx_id
        );
    };

    let log_adapter_notify_name = |cx: &Context, name: &str, topic: &str, data: &Option<serde_json::Value>, cx_id: Option<&str>| {
        info!(
            cx.log(),
            "Adapter '{}' notification received: topic={}, data={:?}, cx_id={:?}",
            name,
            topic,
            data,
            cx_id
        );
    };

    let log_adapter_notify_policy = |cx: &Context, polidy: StartPolicy, topic: &str, data: &Option<serde_json::Value>, cx_id: Option<&str>,| {
        info!(
            cx.log(),
            "Adapter notification with policy {:?}: topic={}, data={:?}, cx_id={:?}",
            polidy,
            topic,
            data,
            cx_id
        );
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
        .on_action_notify(log_action_notify)
        .on_adapter_notify_all(log_adapter_notify)
        .on_adapter_notify_name(log_adapter_notify_name)
        .on_adapter_notify_policy(log_adapter_notify_policy)
        .on_adapter_notify_topic(log_adapter_notify);


    let shared_binds = SharedBindings::default();
    let template_store = TemplateStore::default();
    let active_char = ActiveChar::default();
    let in_combat = InCombat::default();

    let plugin = match
        PluginBuilder::new()
            .hooks(hooks)
            .add_adapter(Gw2BindingsAdapter::new())
            .add_adapter(Gw2ApiAdapter::new())
            .add_adapter(MumbleAdapter::new())
            .add_adapter(Gw2ExecAdapter::new())
            .with_extension(Arc::new(shared_binds))
            .with_extension(Arc::new(template_store))
            .with_extension(Arc::new(active_char))
            .with_extension(Arc::new(in_combat))
            .register_action(
                ActionFactory::new(concat!(PLUGIN_ID, ".set-template"), ||
                    actions::set_template::SetTemplateAction::default()
                )
            )
            .register_action(
                ActionFactory::new(concat!(PLUGIN_ID, ".settings"), ||
                    actions::settings::SettingsAction::default()
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
