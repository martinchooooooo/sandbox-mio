mod activity;
mod agents;
mod backends;
mod bootstrap;
mod commands;
mod credentials;
mod env_ext;
mod error;
mod inference;
mod logging;
mod machine;
mod mio_files;
mod net_policy;
mod mockctl;
mod network;
mod paths;
mod process;
mod provider;
mod proxy;
mod runtime;
mod services;
mod settings;
mod ssh;
mod sync;
mod transport;
mod workspace;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // First statement by requirement: any process spawned before this inherits the launching
    // terminal's console handles, and both `listen_events` and the Mutagen daemon spawn early
    // (`docs/features/podman-process-isolation/02_dev.md`, Phase A).
    let cleared = process::clear_inheritable_handles();

    // Opt-in and mutually exclusive with this app's own file logging (see `logging.rs`): the
    // plugin's `init()` claims the global tracing subscriber, so enabling it silently zeroes every
    // `mio.log`. Default to the file; set `MIO_DEVTOOLS=1` to take the inspector instead.
    #[cfg(all(debug_assertions, feature = "devtools"))]
    let devtools = match std::env::var("MIO_DEVTOOLS").ok().as_deref() == Some("1") {
        true => Some(tauri_plugin_devtools::Builder::default().init()),
        false => None,
    };

    let bootstrap = bootstrap::build();

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    #[cfg(all(debug_assertions, feature = "devtools"))]
    if let Some(devtools) = devtools {
        builder = builder.plugin(devtools);
    }

    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(move |app| bootstrap.register_state(app, cleared))
        .invoke_handler(commands::handler())
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(bootstrap::on_run_event);
}
