//! Parsing of `pi-host.sh ensure` output into a [`HostStatus`], including pending dialogs.

use serde::Deserialize;

use super::types::{AgentHostError, HostStatus};

/// Dialog methods pi blocks on until answered; `notify`, `setStatus` and the rest are fire-and-forget.
const BLOCKING_METHODS: [&str; 4] = ["select", "confirm", "input", "editor"];

#[derive(Deserialize)]
struct HostLine {
    generation: String,
    #[serde(default)]
    pi_version: String,
    #[serde(default)]
    started: bool,
}

#[derive(Deserialize)]
struct UiLine {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    method: String,
}

/// Parses `ensure`'s stdout: one `mio_host` line, then the journal's `extension_ui_request` lines
/// and the input log's `extension_ui_response` lines, in any order.
pub fn parse_ensure_output(stdout: &str) -> Result<HostStatus, AgentHostError> {
    let mut lines = stdout.lines().filter(|l| !l.trim().is_empty());
    let header = lines
        .next()
        .and_then(|l| serde_json::from_str::<HostLine>(l).ok())
        .ok_or_else(|| AgentHostError::NotRunning(format!("unexpected supervisor output: {stdout}")))?;
    Ok(HostStatus {
        generation: header.generation,
        pi_version: header.pi_version,
        started: header.started,
        pending_dialogs: pending_dialogs(lines),
    })
}

/// The blocking dialog requests whose id no response answers, oldest first.
pub fn pending_dialogs<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<serde_json::Value> {
    let mut requests = Vec::new();
    let mut answered = std::collections::HashSet::new();
    for line in lines {
        let Ok(raw) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Ok(ui) = UiLine::deserialize(&raw) else { continue };
        match ui.kind.as_str() {
            "extension_ui_request" if BLOCKING_METHODS.contains(&ui.method.as_str()) => {
                requests.push((ui.id, raw))
            }
            "extension_ui_response" => {
                answered.insert(ui.id);
            }
            _ => {}
        }
    }
    requests.into_iter().filter(|(id, _)| !answered.contains(id)).map(|(_, raw)| raw).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIALOG: &str = include_str!("fixtures/04b-dialog.jsonl");
    const ANSWER: &str = include_str!("fixtures/04d-answer.jsonl");
    const DIALOG2: &str = include_str!("fixtures/04e-dialog2.jsonl");
    const ANSWER2: &str = include_str!("fixtures/04e-answer2.jsonl");
    const FIRST: &str = "ed0d9733-6329-41cc-a685-8441f20fb280";
    const SECOND: &str = "0199fced-9cb8-4665-9738-140cee28f12a";

    fn response(id: &str) -> String {
        format!("{{\"type\":\"extension_ui_response\",\"id\":\"{id}\",\"confirmed\":true}}")
    }

    fn output(journal: &[&str], input: &[String]) -> String {
        let mut out = String::from("{\"type\":\"mio_host\",\"generation\":\"g1\",\"pi_version\":\"1.5.0\",\"started\":false}\n");
        for part in journal {
            out.push_str(&part.replace("\r\n", "\n"));
        }
        for line in input {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    fn ids(status: &HostStatus) -> Vec<&str> {
        status.pending_dialogs.iter().map(|d| d["id"].as_str().unwrap()).collect()
    }

    #[test]
    fn an_unanswered_confirm_is_pending() {
        let status = parse_ensure_output(&output(&[DIALOG], &[])).unwrap();
        assert_eq!(status.generation, "g1");
        assert_eq!(status.pi_version, "1.5.0");
        assert_eq!(ids(&status), [FIRST]);
    }

    #[test]
    fn an_answered_dialog_is_not_pending_and_notify_never_is() {
        let status = parse_ensure_output(&output(&[DIALOG, ANSWER], &[response(FIRST)])).unwrap();
        assert!(status.pending_dialogs.is_empty());
    }

    #[test]
    fn only_the_second_dialog_stays_pending_after_the_first_is_answered() {
        let status =
            parse_ensure_output(&output(&[DIALOG, ANSWER, DIALOG2], &[response(FIRST)])).unwrap();
        assert_eq!(ids(&status), [SECOND]);
        let status = parse_ensure_output(&output(
            &[DIALOG, ANSWER, DIALOG2, ANSWER2],
            &[response(FIRST), response(SECOND)],
        ))
        .unwrap();
        assert!(status.pending_dialogs.is_empty());
    }

    #[test]
    fn output_without_a_host_line_is_an_error() {
        assert!(matches!(parse_ensure_output(""), Err(AgentHostError::NotRunning(_))));
    }
}
