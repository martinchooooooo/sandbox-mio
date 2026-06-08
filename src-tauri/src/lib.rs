use bollard::container::{Config, CreateContainerOptions, ListContainersOptions, StartContainerOptions, StopContainerOptions};
use bollard::system::EventsOptions;
use bollard::{Docker, API_DEFAULT_VERSION};
use futures_util::StreamExt;
use serde::Serialize;
use tauri::{ipc::Channel, State};

fn podman_pipe() -> String {
    if let Ok(pipe) = std::env::var("PODMAN_PIPE") {
        return pipe;
    }
    let machine = std::env::var("PODMAN_MACHINE")
        // .unwrap_or_else(|_| "podman-machine-default".to_string());
        .unwrap_or_else(|_| "podman-mio-tests".to_string());
    format!("//./pipe/{}", machine)
}

#[derive(Debug, Serialize, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub status: String,
}

pub struct PodmanProjectStore;

impl PodmanProjectStore {
    fn client(&self) -> Result<Docker, String> {
        let pipe = podman_pipe();
        eprintln!("[podman] connecting to pipe: {pipe}");
        Docker::connect_with_named_pipe(&pipe, 120, API_DEFAULT_VERSION)
            .map_err(|e| {
                eprintln!("[podman] connect error: {e}");
                e.to_string()
            })
    }

    pub async fn create(&self, name: &str) -> Result<Project, String> {
        let docker = self.client()?;
        let response = docker
            .create_container(
                Some(CreateContainerOptions {
                    name,
                    platform: None,
                }),
                Config {
                    image: Some("alpine:latest"),
                    cmd: Some(vec!["/bin/sh"]),
                    tty: Some(true),
                    open_stdin: Some(true),
                    attach_stdin: Some(true),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| e.to_string())?;

        Ok(Project {
            id: response.id,
            name: name.to_string(),
            status: "created".to_string(),
        })
    }

    pub async fn list(&self) -> Result<Vec<Project>, String> {
        let docker = self.client()?;
        let containers = docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            }))
            .await
            .map_err(|e| {
                eprintln!("[podman] list_containers error: {e}");
                e.to_string()
            })?;

        Ok(containers
            .into_iter()
            .map(|c| Project {
                id: c.id.unwrap_or_default(),
                name: c.names
                    .unwrap_or_default()
                    .into_iter()
                    .next()
                    .unwrap_or_default()
                    .trim_start_matches('/')
                    .to_string(),
                status: c.status.unwrap_or_default(),
            })
            .collect())
    }

    pub async fn start(&self, name: &str) -> Result<(), String> {
        let docker = self.client()?;
        docker.start_container(name, None::<StartContainerOptions<String>>).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn stop(&self, name: &str) -> Result<(), String> {
        let docker = self.client()?;
        docker.stop_container(name, None::<StopContainerOptions>).await.map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn create_project(
    name: String,
    store: State<'_, PodmanProjectStore>,
) -> Result<Project, String> {
    store.create(&name).await
}

#[tauri::command]
async fn list_projects(store: State<'_, PodmanProjectStore>) -> Result<Vec<Project>, String> {
    store.list().await
}

#[tauri::command]
async fn start_project(name: String, store: State<'_, PodmanProjectStore>) -> Result<(), String> {
    store.start(&name).await
}

#[tauri::command]
async fn stop_project(name: String, store: State<'_, PodmanProjectStore>) -> Result<(), String> {
    store.stop(&name).await
}

#[tauri::command]
async fn listen_events(on_event: Channel<serde_json::Value>) -> Result<(), String> {
    let docker = Docker::connect_with_named_pipe(&podman_pipe(), 120, API_DEFAULT_VERSION)
        .map_err(|e| e.to_string())?;
    let mut stream = docker.events(Some(EventsOptions::<String>::default()));

    while let Some(event) = stream.next().await {
        match event {
            Ok(ev) => {
                let value = serde_json::to_value(ev).map_err(|e| e.to_string())?;
                on_event.send(value).map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(debug_assertions)]
    let devtools = tauri_plugin_devtools::Builder::default().init();

    let mut builder = tauri::Builder::default();

    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(devtools);
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .manage(PodmanProjectStore)
        .invoke_handler(tauri::generate_handler![greet, create_project, list_projects, start_project, stop_project, listen_events])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
