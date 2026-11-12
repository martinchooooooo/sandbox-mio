//! `MockAgentHost`: replays the spike's recorded pi traces under `MIO_MOCK_BACKEND=1`, so the
//! `light` tier exercises the chat without an engine or a model.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::ipc::Channel;
use tokio::sync::Mutex;

use super::types::{AgentHost, AgentHostError, AttachEvent, AttachInfo, Cursor, HostStatus};
use crate::provider::types::MachineRef;

const ROUNDTRIP: &str = include_str!("fixtures/01-roundtrip.jsonl");
const DIALOG: &str = include_str!("fixtures/04b-dialog.jsonl");
const ANSWER: &str = include_str!("fixtures/04d-answer.jsonl");
/// A prompt containing this replays the dialog traces instead of the round trip.
const DIALOG_TRIGGER: &str = "/spikeconfirm";
/// Compressed replay timing: the recorded round trip took 6.5 s.
const RECORD_DELAY: Duration = Duration::from_millis(8);

#[derive(Default)]
struct Sandbox {
    running: bool,
    generation: u64,
    offset: u64,
    channel: Option<Channel<AttachEvent>>,
    history: Vec<Value>,
    pending: Vec<Value>,
    /// The id of the prompt whose dialog is pending, answered once the dialog is.
    dialog_prompt: Option<Value>,
    thinking_level: String,
    model_id: String,
}

#[derive(Default)]
pub struct MockAgentHost {
    sandboxes: Arc<Mutex<std::collections::HashMap<String, Sandbox>>>,
}

fn records(fixture: &str) -> Vec<Value> {
    fixture.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn models() -> Value {
    let model = |id: &str, name: &str| {
        json!({
            "id": id, "name": name, "api": "openai-completions", "provider": "local-llm",
            "baseUrl": "http://127.0.0.1:51199/v1", "reasoning": true, "input": ["text"],
            "contextWindow": 32768, "maxTokens": 16384
        })
    };
    json!([model("qwen3.5-2b", "Qwen3.5 2B"), model("qwen3.5-4b", "Qwen3.5 4B")])
}

impl Sandbox {
    fn emit(&mut self, record: Value) {
        let len = serde_json::to_vec(&record).map_or(0, |b| b.len() as u64 + 1);
        let event = AttachEvent { offset: Some(self.offset), end: Some(self.offset + len), record };
        self.offset += len;
        if let Some(channel) = &self.channel {
            let _ = channel.send(event);
        }
    }

    fn respond(&mut self, id: &Value, command: &str, data: Value) {
        self.emit(json!({ "id": id, "type": "response", "command": command, "success": true, "data": data }));
    }

    fn state(&self) -> Value {
        let model = models()
            .as_array()
            .and_then(|m| m.iter().find(|m| m["id"] == self.model_id.as_str()).cloned())
            .unwrap_or(Value::Null);
        json!({
            "model": model, "thinkingLevel": self.thinking_level, "isStreaming": false,
            "isCompacting": false, "steeringMode": "one-at-a-time", "followUpMode": "one-at-a-time",
            "sessionFile": "/root/work/.mio/pi-agent/chat-sessions/--root-work--/mock.jsonl",
            "sessionId": "mock-session", "autoCompactionEnabled": true,
            "messageCount": self.history.len(), "pendingMessageCount": 0
        })
    }
}

impl MockAgentHost {
    /// Emits `items` one by one with the compressed delay, re-locking per record so commands sent
    /// mid-replay are still answered.
    fn replay(&self, name: &str, items: Vec<Value>) {
        let sandboxes = self.sandboxes.clone();
        let name = name.to_string();
        tokio::spawn(async move {
            for record in items {
                tokio::time::sleep(RECORD_DELAY).await;
                if let Some(sb) = sandboxes.lock().await.get_mut(&name) {
                    sb.emit(record);
                }
            }
        });
    }

    async fn handle(&self, name: &str, command: Value) {
        let id = command["id"].clone();
        let kind = command["type"].as_str().unwrap_or_default().to_string();
        let mut sandboxes = self.sandboxes.lock().await;
        let Some(sb) = sandboxes.get_mut(name) else { return };
        match kind.as_str() {
            "get_state" => {
                let state = sb.state();
                sb.respond(&id, "get_state", state);
            }
            "get_messages" => {
                let messages = Value::Array(sb.history.clone());
                sb.respond(&id, "get_messages", json!({ "messages": messages }));
            }
            "get_available_models" => sb.respond(&id, &kind, json!({ "models": models() })),
            "get_available_thinking_levels" => sb.respond(
                &id,
                &kind,
                json!({ "levels": ["off", "minimal", "low", "medium", "high"] }),
            ),
            "set_thinking_level" => {
                sb.thinking_level = command["level"].as_str().unwrap_or("off").to_string();
                sb.respond(&id, &kind, Value::Null);
            }
            "set_model" => {
                sb.model_id = command["modelId"].as_str().unwrap_or_default().to_string();
                let model = sb.state()["model"].clone();
                sb.respond(&id, &kind, model);
            }
            "new_session" | "switch_session" => {
                sb.history.clear();
                sb.respond(&id, &kind, json!({ "cancelled": false }));
            }
            "extension_ui_response" => {
                let answered = command["id"].clone();
                sb.pending.retain(|p| p["id"] != answered);
                if let Some(prompt_id) = sb.dialog_prompt.take() {
                    let mut items = records(ANSWER);
                    if let Some(last) = items.iter_mut().find(|r| r["type"] == "response") {
                        last["id"] = prompt_id;
                    }
                    drop(sandboxes);
                    self.replay(name, items);
                }
            }
            "prompt" | "steer" | "follow_up" => {
                let text = command["message"].as_str().unwrap_or_default().to_string();
                if kind == "prompt" && text.contains(DIALOG_TRIGGER) {
                    let dialog = records(DIALOG);
                    sb.pending.extend(dialog.iter().cloned());
                    sb.dialog_prompt = Some(id);
                    drop(sandboxes);
                    self.replay(name, dialog);
                    return;
                }
                sb.respond(&id, &kind, json!({ "disposition": "started" }));
                if kind != "prompt" {
                    return;
                }
                let items: Vec<Value> =
                    records(ROUNDTRIP).into_iter().filter(|r| r["type"] != "response").collect();
                sb.history.push(json!({ "role": "user", "content": [{ "type": "text", "text": text }] }));
                if let Some(end) = items
                    .iter()
                    .rev()
                    .find(|r| r["type"] == "message_end" && r["message"]["role"] == "assistant")
                {
                    sb.history.push(end["message"].clone());
                }
                drop(sandboxes);
                self.replay(name, items);
            }
            _ => sb.respond(&id, &kind, Value::Null),
        }
    }
}

#[async_trait::async_trait]
impl AgentHost for MockAgentHost {
    async fn ensure(&self, name: &str, _machine: &MachineRef) -> Result<HostStatus, AgentHostError> {
        let mut sandboxes = self.sandboxes.lock().await;
        let sb = sandboxes.entry(name.to_string()).or_default();
        let started = !sb.running;
        if started {
            *sb = Sandbox {
                running: true,
                generation: sb.generation + 1,
                thinking_level: "off".into(),
                model_id: "qwen3.5-2b".into(),
                ..Default::default()
            };
        }
        Ok(HostStatus {
            generation: format!("mock-{}", sb.generation),
            pi_version: PI_VERSION.to_string(),
            started,
            pending_dialogs: sb.pending.clone(),
        })
    }

    async fn attach(
        &self,
        name: &str,
        _machine: &MachineRef,
        _cursor: Option<Cursor>,
        on_event: Channel<AttachEvent>,
    ) -> Result<AttachInfo, AgentHostError> {
        let mut sandboxes = self.sandboxes.lock().await;
        let sb = sandboxes.get_mut(name).ok_or_else(|| AgentHostError::NotRunning("not ensured".into()))?;
        sb.channel = Some(on_event);
        Ok(AttachInfo { generation: format!("mock-{}", sb.generation), offset: sb.offset, writer: true })
    }

    async fn send(&self, name: &str, line: String) -> Result<(), AgentHostError> {
        let command: Value =
            serde_json::from_str(&line).map_err(|e| AgentHostError::Stream(e.to_string()))?;
        self.handle(name, command).await;
        Ok(())
    }

    async fn detach(&self, name: &str) {
        if let Some(sb) = self.sandboxes.lock().await.get_mut(name) {
            if let Some(channel) = sb.channel.take() {
                let _ = channel.send(super::detached_event("detached"));
            }
        }
    }

    async fn detach_all(&self) {
        for sb in self.sandboxes.lock().await.values_mut() {
            sb.channel = None;
        }
    }

    async fn takeover(&self, _name: &str, _machine: &MachineRef) -> Result<(), AgentHostError> {
        Ok(())
    }

    async fn stop(&self, name: &str, _machine: &MachineRef) -> Result<(), AgentHostError> {
        self.detach(name).await;
        if let Some(sb) = self.sandboxes.lock().await.get_mut(name) {
            sb.running = false;
        }
        Ok(())
    }
}

/// The pi version the mock reports; matches the recorded traces.
const PI_VERSION: &str = "1.5.0";
