pub mod types;

use bollard::container::{Config, CreateContainerOptions, ListContainersOptions, StartContainerOptions, StopContainerOptions};
use bollard::image::CreateImageOptions;
use bollard::system::EventsOptions;
use bollard::{Docker, API_DEFAULT_VERSION};
use futures_util::StreamExt;
use tauri::ipc::Channel;

use types::{PodmanBackend, PodmanBollardBackend, PodmanShellBackend, Sandbox, SandboxBackend};

pub fn default_backend() -> std::sync::Arc<dyn SandboxBackend> {
    std::sync::Arc::new(PodmanBollardBackend)
}

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

        let image = format!("{}:{}", Self::base_image(), Self::base_image_version());
        let response = docker
            .create_container(
                Some(CreateContainerOptions {
                    name,
                    platform: None,
                }),
                Config {
                    image: Some(image.as_str()),
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
