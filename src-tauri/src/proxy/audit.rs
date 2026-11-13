//! iron-proxy's audit stream as the traffic record
//! (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 4). The sidecar's
//! `k8s-file` log is truncated to zero at its size cap, so it is only a transport: each poll
//! translates the lines logged since `audit.cursor` and appends them to the host-side
//! `flows.ndjson`, which [`super::flowlog`] reads exactly as it reads a mitmproxy log.
//!
//! The record shape is iron-proxy 0.56.0's, captured in
//! `docs/features/podman-egress-boundary/spike/fixtures/podman-logs-timestamps.txt`; the README's
//! flat example is wrong for that version.

use std::io::Write;
use std::path::Path;

use serde_json::Value;

use super::flowlog::{FlowEntry, FLOW_LOG_MAX_LINES};
use crate::provider::types::MachineProvider;

const CURSOR_FILE: &str = "audit.cursor";

/// The newest `podman logs` timestamp ingested, and how many lines carrying exactly that stamp were
/// ingested: Podman stamps every line of one read with the same value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditCursor {
    since: String,
    seen: usize,
}

/// One audit line's JSON body (the `podman logs` timestamp prefix already removed) as a traffic
/// row, or `None` for anything that is not a request worth a row. Advances `next_id` only when a
/// row is produced.
pub fn translate(line: &str, next_id: &mut u64) -> Option<FlowEntry> {
    let record: Value = serde_json::from_str(line).ok()?;
    if record["msg"] != "request" {
        return None;
    }
    let audit = &record["audit"];
    let method = audit["method"].as_str()?.to_string();
    let rejected_by = record["rejected_by"].as_str().map(str::to_string);
    // An allowed CONNECT is followed by the inner request's own row, which carries the path and
    // status; a rejected one has no inner row.
    if method == "CONNECT" && rejected_by.is_none() && audit["action"] == "allow" {
        return None;
    }
    let transform = |name: &str| {
        record["request_transforms"]
            .as_array()
            .and_then(|ts| ts.iter().find(|t| t["name"] == name))
            .map(|t| &t["annotations"])
    };
    let would_block = transform("allowlist").is_some_and(|a| a["action"] == "warn");
    let secret_injected = transform("secrets")
        .and_then(|a| a["swapped"].as_array())
        .is_some_and(|swapped| !swapped.is_empty());
    let ts = record["time"].as_str().and_then(parse_time)?.unix_timestamp_nanos() as f64 / 1e9;
    *next_id += 1;
    Some(FlowEntry {
        id: *next_id,
        method,
        host: audit["host"].as_str().unwrap_or_default().to_string(),
        path: audit["path"].as_str().unwrap_or_default().to_string(),
        status: audit["status_code"]
            .as_u64()
            .and_then(|s| u16::try_from(s).ok())
            .filter(|s| *s != 0),
        ts,
        duration_ms: audit["duration_ms"].as_f64(),
        req_bytes: None,
        resp_bytes: None,
        secret_injected,
        error: record["error"].as_str().map(str::to_string),
        rejected_by,
        would_block,
    })
}

fn parse_time(stamp: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(stamp, &time::format_description::well_known::Rfc3339).ok()
}

/// Splits `podman logs --timestamps` lines into the bodies not yet ingested under `cursor`, and
/// returns the cursor that covers all of `lines`. Podman 5.8 truncates `--since` to whole seconds,
/// so a poll also repeats the lines logged earlier in the cursor's second.
fn unseen<'a>(
    lines: &'a [String],
    cursor: Option<&AuditCursor>,
) -> (Vec<&'a str>, Option<AuditCursor>) {
    let stamped: Vec<(&str, &str)> = lines.iter().filter_map(|l| l.split_once(' ')).collect();
    let since = cursor.and_then(|c| parse_time(&c.since));
    let mut skip = cursor.map_or(0, |c| c.seen);
    let mut bodies = Vec::new();
    for (stamp, body) in &stamped {
        if let Some(since) = since {
            match parse_time(stamp).map(|t| t.cmp(&since)) {
                Some(std::cmp::Ordering::Less) => continue,
                Some(std::cmp::Ordering::Equal) if skip > 0 => {
                    skip -= 1;
                    continue;
                }
                _ => {}
            }
        }
        bodies.push(*body);
    }
    let next = stamped.last().map(|(last, _)| AuditCursor {
        since: last.to_string(),
        seen: stamped.iter().filter(|(s, _)| s == last).count(),
    });
    (bodies, next.or_else(|| cursor.cloned()))
}

/// Pulls the sidecar's audit lines logged since the last poll into `flows.ndjson` under
/// `state_dir`. Returns whether the file was rewritten to its newest [`FLOW_LOG_MAX_LINES`] rows,
/// which invalidates a reader's byte offset. Does nothing once `flows.ndjson` is gone, so a poll
/// racing `remove_proxy` never recreates the state directory.
pub async fn ingest(provider: &dyn MachineProvider, sidecar: &str, state_dir: &Path) -> bool {
    let flows = state_dir.join("flows.ndjson");
    let cursor_path = state_dir.join(CURSOR_FILE);
    let cursor: Option<AuditCursor> =
        crate::mio_files::read_value(&cursor_path).and_then(|v| serde_json::from_value(v).ok());
    let Ok(lines) =
        provider.sidecar_logs(sidecar, cursor.as_ref().map(|c| c.since.as_str()), None).await
    else {
        return false;
    };
    let (bodies, next) = unseen(&lines, cursor.as_ref());
    if next == cursor {
        return false;
    }
    let Ok(existing) = std::fs::read_to_string(&flows) else {
        return false;
    };
    let mut next_id = existing
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<FlowEntry>(l).ok())
        .map_or(0, |e| e.id);
    let rows: Vec<String> = bodies
        .iter()
        .filter_map(|b| translate(b, &mut next_id))
        .filter_map(|e| serde_json::to_string(&e).ok())
        .collect();
    let rewritten = match append(&flows, &existing, &rows) {
        Ok(rewritten) => rewritten,
        Err(e) => {
            tracing::warn!(target: "proxy", %sidecar, error = %e, "audit ingest failed");
            return false;
        }
    };
    if let Some(next) = next {
        if let Err(e) = crate::mio_files::write_compact(&cursor_path, &serde_json::json!(next)) {
            tracing::warn!(target: "proxy", %sidecar, error = %e, "audit cursor not saved");
        }
    }
    rewritten
}

/// Appends `rows`, or, once the file would pass the cap, atomically replaces it with its newest
/// [`FLOW_LOG_MAX_LINES`] rows (the cap `mitm_flowlog_addon.py` keeps).
fn append(flows: &Path, existing: &str, rows: &[String]) -> std::io::Result<bool> {
    if rows.is_empty() {
        return Ok(false);
    }
    let kept = existing.lines().count();
    if kept + rows.len() <= FLOW_LOG_MAX_LINES {
        let mut file = std::fs::OpenOptions::new().append(true).open(flows)?;
        file.write_all((rows.join("\n") + "\n").as_bytes())?;
        return Ok(false);
    }
    let all: Vec<&str> = existing.lines().chain(rows.iter().map(String::as_str)).collect();
    let newest = &all[all.len().saturating_sub(FLOW_LOG_MAX_LINES)..];
    let tmp = flows.with_extension("ndjson.tmp");
    std::fs::write(&tmp, newest.join("\n") + "\n")?;
    std::fs::rename(&tmp, flows)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Copied from `docs/features/podman-egress-boundary/spike/fixtures/podman-logs-timestamps.txt`.
    const FIXTURE: &str = include_str!("fixtures/podman-logs-timestamps.txt");

    #[test]
    fn translates_the_captured_audit_stream() {
        let lines: Vec<String> = FIXTURE.lines().map(str::to_string).collect();
        let (bodies, _) = unseen(&lines, None);
        let mut id = 0;
        let rows: Vec<FlowEntry> = bodies.iter().filter_map(|b| translate(b, &mut id)).collect();
        let summary: Vec<(&str, &str, Option<u16>)> =
            rows.iter().map(|r| (r.method.as_str(), r.host.as_str(), r.status)).collect();
        assert_eq!(
            summary,
            [
                ("GET", "example.com", Some(200)),
                ("GET", "example.com", Some(200)),
                ("GET", "example.com", Some(200)),
                ("GET", "www.wikipedia.org", Some(200)),
                ("CONNECT", "www.wikipedia.org:443", Some(403)),
                ("GET", "www.wikipedia.org", Some(403)),
                ("GET", "httpbin.org", Some(200)),
                ("GET", "httpbin.org", Some(200)),
                ("GET", "httpbin.org", Some(200)),
                ("GET", "10.88.0.1.nip.io", Some(502)),
                ("GET", "httpbin.org", Some(200)),
            ],
            "allowed CONNECT rows and non-request lines are dropped"
        );
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), (1..=11).collect::<Vec<_>>());
        assert!(rows[3].would_block && rows.iter().filter(|r| r.would_block).count() == 1);
        assert_eq!(rows[4].rejected_by.as_deref(), Some("allowlist"));
        assert_eq!(rows[4].path, "");
        assert_eq!(rows[5].rejected_by.as_deref(), Some("allowlist"));
        let swapped: Vec<&str> =
            rows.iter().filter(|r| r.secret_injected).map(|r| r.path.as_str()).collect();
        assert_eq!(swapped, ["/anything/h", "/anything/q"]);
        assert!(rows[9].error.as_deref().is_some_and(|e| e.contains("upstream_deny_cidrs")));
        assert!(rows.iter().all(|r| r.req_bytes.is_none() && r.resp_bytes.is_none()));
        assert!(
            (rows[0].ts - 1_794_566_344.076_225).abs() < 1e-3,
            "ts is the record's own UTC time"
        );
    }

    #[test]
    fn a_repeated_poll_skips_lines_already_ingested() {
        let lines: Vec<String> = FIXTURE.lines().map(str::to_string).collect();
        // Lines 10 and 11 share one stamp; the first poll ends between them. The second poll gets
        // the whole log back, as `--since` truncated to the second can return earlier lines.
        let (_, cursor) = unseen(&lines[..10], None);
        let (bodies, _) = unseen(&lines, cursor.as_ref());
        assert_eq!(bodies.len(), lines.len() - 10);
        assert!(bodies[0].contains(r#""rejected_by":"allowlist""#));
    }
}
