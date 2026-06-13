use bollard::container::{Config, CreateContainerOptions, ListContainersOptions, StartContainerOptions, StopContainerOptions};
use bollard::image::CreateImageOptions;
use bollard::system::EventsOptions;
use bollard::{Docker, API_DEFAULT_VERSION};
use futures_util::StreamExt;
use tauri::ipc::Channel;

use crate::{Sandbox};

/// Primary interface for sandbox lifecycle management. Tauri state holds an
/// `Arc<dyn SandboxBackend>` — inject that to call any of these methods.
#[async_trait::async_trait]
pub trait SandboxBackend: Send + Sync {
    /// Pull the base image if absent, then create a stopped container.
    async fn create(&self, name: &str) -> Result<Sandbox, String>;
    /// Return all containers (running and stopped).
    async fn list(&self) -> Result<Vec<Sandbox>, String>;
    /// Start a stopped container.
    async fn start(&self, name: &str) -> Result<(), String>;
    /// Stop a running container.
    async fn stop(&self, name: &str) -> Result<(), String>;
    /// Stream container events to the frontend channel until the connection drops.
    async fn listen_events(&self, on_event: Channel<serde_json::Value>) -> Result<(), String>;
}

// region: Podman Traits

/// Internal helper trait shared by both Podman backends. Not part of the public API.
trait PodmanBackend {
    /// Returns the Windows named-pipe path for the Podman socket.
    /// Override entirely by setting `PODMAN_PIPE`; otherwise derived from `podman_machine()`.
    fn podman_pipe(&self) -> String {
        if let Ok(pipe) = std::env::var("PODMAN_PIPE") {
            return pipe;
        }
        let mut machine = self.podman_machine();
        // Only podman-machine-default seems to carry the extra prefix
        if !machine.starts_with("podman-") {
            machine = format!("podman-{machine}")
        }
        format!("//./pipe/{}", machine)
    }

    /// Returns the Podman machine name, defaulting to `podman-machine-default`.
    /// Override by setting `PODMAN_MACHINE` (the E2E harness sets this to `mio-tests`).
    fn podman_machine(&self) -> String {
        let machine = std::env::var("PODMAN_MACHINE")
            .unwrap_or_else(|_| "podman-machine-default".to_string());
        return machine
    }

    /// Base image used when creating sandboxes. Override in a subtype to change the image.
    fn base_image() -> &'static str { "alpine" }
    /// Tag for `base_image`. Override in a subtype to pin a specific version.
    fn base_image_version() -> &'static str { "latest" }
}

// endregion

// region: PodmanBollard

/// Talks to Podman via the Docker-compatible API over a Windows named pipe.
/// A new connection is opened on each call — there is no persistent client.
pub struct PodmanBollardBackend;

impl PodmanBackend for PodmanBollardBackend {}

impl PodmanBollardBackend {
    fn client(&self) -> Result<Docker, String> {
        let pipe = self.podman_pipe();
        eprintln!("[podman] connecting to pipe: {pipe}");
        Docker::connect_with_named_pipe(&pipe, 120, API_DEFAULT_VERSION)
            .map_err(|e| {
                eprintln!("[podman] connect error: {e}");
                e.to_string()
            })
    }
}

#[async_trait::async_trait]
impl SandboxBackend for PodmanBollardBackend {
    async fn create(&self, name: &str) -> Result<Sandbox, String> {
        let docker = self.client()?;

        let mut pull = docker.create_image(
            Some(CreateImageOptions {
                from_image: Self::base_image(),
                tag:  Self::base_image_version(),
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(result) = pull.next().await {
            result.map_err(|e| e.to_string())?;
        }

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

        Ok(Sandbox {
            id: response.id,
            name: name.to_string(),
            status: "created".to_string(),
        })
    }

    async fn list(&self) -> Result<Vec<Sandbox>, String> {
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
            .map(|c| Sandbox {
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

    async fn start(&self, name: &str) -> Result<(), String> {
        let docker = self.client()?;
        docker.start_container(name, None::<StartContainerOptions<String>>).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<(), String> {
        let docker = self.client()?;
        docker.stop_container(name, None::<StopContainerOptions>).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn listen_events(&self, on_event: Channel<serde_json::Value>) -> Result<(), String> {
        let docker = self.client()?;
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
}

// endregion

// region: PodmanShell

/// Drives Podman by SSHing into the Podman machine and running CLI commands.
/// Use this when the named pipe is not accessible. Each operation spawns a subprocess.
pub struct PodmanShellBackend;

impl PodmanBackend for PodmanShellBackend {}

impl PodmanShellBackend {
    async fn ssh(&self, cmd: &str) -> Result<std::process::Output, String> {
        tokio::process::Command::new("podman")
            .args(["machine", "ssh", &self.podman_machine(), cmd])
            .output()
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait::async_trait]
impl SandboxBackend for PodmanShellBackend {
    async fn create(&self, name: &str) -> Result<Sandbox, String> {
        let image = format!("{}:{}", Self::base_image(), Self::base_image_version());
        let cmd = format!("podman create --name {name} {image} /bin/sh");
        let out = self.ssh(&cmd).await?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(Sandbox { id, name: name.to_string(), status: "created".to_string() })
    }

    async fn list(&self) -> Result<Vec<Sandbox>, String> {
        let out = self.ssh("podman ps --all --format json").await?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        let containers: Vec<serde_json::Value> =
            serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
        Ok(containers
            .into_iter()
            .map(|c| Sandbox {
                id: c["Id"].as_str().unwrap_or_default().to_string(),
                name: c["Names"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim_start_matches('/')
                    .to_string(),
                status: c["State"].as_str().unwrap_or_default().to_string(),
            })
            .collect())
    }

    async fn start(&self, name: &str) -> Result<(), String> {
        let out = self.ssh(&format!("podman start {name}")).await?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<(), String> {
        let out = self.ssh(&format!("podman stop {name}")).await?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        Ok(())
    }

    async fn listen_events(&self, on_event: Channel<serde_json::Value>) -> Result<(), String> {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut child = tokio::process::Command::new("podman")
            .args(["machine", "ssh", &self.podman_machine(), "podman events --format json"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;

        let stdout = child.stdout.take().ok_or_else(|| "no stdout".to_string())?;
        let mut lines = BufReader::new(stdout).lines();

        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            let value: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            on_event.send(value).map_err(|e| e.to_string())?;
        }

        Ok(())
    }
}

// endregion
