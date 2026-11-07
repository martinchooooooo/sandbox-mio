//! The Tauri command surface: one file per domain, plus the `generate_handler!` inventory.

pub mod agents;
pub mod app;
pub mod credentials;
pub mod deploy;
pub mod hosts;
pub mod image;
pub mod inference;
pub mod machine;
pub mod mockctl;
pub mod net_policy;
pub mod network;
pub mod packages;
pub mod panes;
pub mod proxy;
pub mod sandbox;

/// The whole `#[tauri::command]` inventory, registered once by `run()`. A command that compiles but
/// is missing here is rejected at `invoke()` with "command not found".
///
/// Pinned to `tauri::Wry` rather than generic over `R: tauri::Runtime`: commands taking a concrete
/// `tauri::AppHandle` are `CommandArg` only for that runtime
/// (`docs/features/lib-decomposition/02_dev.md`, Resolved Decision 5).
pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        image::base_image_version,
        image::build_base_image,
        image::base_image_ready,
        image::provider_capabilities,
        image::get_dismissed_image_version,
        image::dismiss_image_update,
        app::mutagen_version,
        sandbox::create_sandbox,
        packages::install_package,
        packages::uninstall_package,
        panes::open_terminal,
        panes::watch_workspace_ports,
        panes::open_terminal_pane,
        panes::open_agent_web,
        panes::switch_app,
        panes::switch_pane,
        panes::hide_active_pane,
        panes::set_panel_collapsed,
        panes::close_workspace,
        panes::close_pane,
        panes::workspace_state,
        packages::list_packages,
        sandbox::list_sandboxes,
        sandbox::start_sandbox,
        sandbox::stop_sandbox,
        deploy::deploy_sandbox,
        deploy::undeploy_sandbox,
        sandbox::destroy_sandbox,
        deploy::sandbox_deployments,
        network::list_open_forwards,
        network::expose_port,
        network::unexpose_port,
        machine::check_podman_ready,
        machine::machine_needs_init,
        machine::start_podman_machine,
        machine::provision_podman,
        machine::machine_setup_steps,
        machine::machine_readiness,
        machine::prepare_machine,
        machine::reset_stuck_podman_machine,
        sandbox::listen_events,
        hosts::list_sandbox_hosts,
        hosts::add_sandbox_host,
        hosts::prepare_sandbox_host,
        hosts::provision_sandbox_host,
        deploy::sync_status,
        deploy::enable_sync,
        deploy::disable_sync,
        packages::list_registry,
        agents::install_agent,
        agents::provision_opencode,
        agents::provision_pi,
        agents::start_agent_auth,
        agents::send_agent_auth_input,
        agents::cancel_agent_auth,
        agents::get_agent_auth_status,
        credentials::ensure_auth_container,
        credentials::capture_agent_credentials,
        credentials::inject_agent_credentials,
        credentials::seed_agent_onboarding,
        credentials::list_credential_bundles,
        credentials::delete_credential_bundle,
        credentials::set_github_token,
        inference::detect_machine_specs,
        inference::redetect_machine_specs,
        inference::download_model,
        app::list_tasks,
        inference::downloaded_models,
        inference::inference_setup_steps,
        inference::start_inference,
        inference::stop_inference,
        inference::inference_status,
        inference::install_foundry_local,
        inference::test_model,
        app::get_settings,
        app::set_settings,
        mockctl::mock_configure,
        mockctl::mock_reset,
        mockctl::mock_calls,
        proxy::proxy_status,
        proxy::list_proxy_rules,
        proxy::set_proxy_rules,
        proxy::set_proxy_secret,
        proxy::delete_proxy_secret,
        proxy::restart_proxy,
        proxy::watch_proxy_flows,
        net_policy::get_net_policy,
        net_policy::set_net_policy,
        net_policy::starter_net_policy,
        net_policy::net_policy_drift,
        net_policy::reapply_sandbox_network,
        net_policy::net_policy_violations
    ]
}
