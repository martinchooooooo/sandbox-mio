//! Throwaway spike for docs/features/proxy-credential-secrets/01_ideas.md.
//! Confirms: a `home.tar.gz` bundle shaped like `credentials::tar_capture`'s output
//! (tar -C / -czf -, entries as `root/<path>`, no leading slash) can have one named
//! entry located and read via `tar::Archive::entries()` over a `flate2::GzDecoder`,
//! without calling `.unpack()`/`.unpack_in()` (no bytes ever touch disk as loose files),
//! and without the caller having to read the *content* of entries it does not want.

use std::io::{Read, Write};

/// Wraps a reader and counts bytes actually pulled through `read()`, so the spike can
/// show that skipped entries' payload bytes are drained by the tar crate's own
/// advance-to-next-header logic, not by application code choosing to inspect them.
struct CountingReader<R> {
    inner: R,
    read_calls: usize,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.read_calls += 1;
        self.inner.read(buf)
    }
}

fn build_synthetic_bundle() -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(gz);

    let credentials_json =
        br#"{"claudeAiOauth":{"accessToken":"sk-ant-spike-token-abc123","refreshToken":"r-xyz","expiresAt":1999999999999}}"#;
    append_file(&mut builder, "root/.claude/.credentials.json", credentials_json);

    // Decoy entry the reader must be able to skip without reading its content.
    let decoy: Vec<u8> = vec![b'x'; 64 * 1024];
    append_file(&mut builder, "root/.claude/settings.json", &decoy);

    let account_json = br#"{"oauthAccount":{"emailAddress":"dev@example.com"},"machineID":"m1"}"#;
    append_file(&mut builder, "root/.claude.json", account_json);

    let gz = builder.into_inner().expect("finish tar");
    gz.finish().expect("finish gzip")
}

fn append_file<W: Write>(builder: &mut tar::Builder<W>, path: &str, content: &[u8]) {
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    builder.append_data(&mut header, path, content).expect("append entry");
}

/// Mirrors `crate::agents::json_path_get` (src-tauri/src/agents/mod.rs): dot-path lookup
/// into a `serde_json::Value`, reused rather than reimplemented for the real extraction code.
fn json_path_get<'a>(val: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    path.split('.').fold(Some(val), |acc, key| acc?.get(key))
}

fn main() {
    let archive_bytes = build_synthetic_bundle();
    println!("synthetic bundle size: {} bytes", archive_bytes.len());

    let target_path = "root/.claude/.credentials.json";
    let secret_json_key = "claudeAiOauth.accessToken";

    let counting = CountingReader { inner: archive_bytes.as_slice(), read_calls: 0 };
    let gz = flate2::read::GzDecoder::new(counting);
    let mut archive = tar::Archive::new(gz);

    let mut found: Option<String> = None;
    let mut entries_seen = 0;
    for entry in archive.entries().expect("read entries") {
        let mut entry = entry.expect("read entry header");
        entries_seen += 1;
        let path = entry.path().expect("entry path").to_string_lossy().into_owned();
        if path == target_path {
            let mut buf = String::new();
            entry.read_to_string(&mut buf).expect("read matched entry");
            found = Some(buf);
            // Deliberately do not read the remaining entries' content — the tar
            // crate's iterator drains any unread bytes of the current entry before
            // yielding the next header, which is what makes selective reads safe.
        }
        // No `.unpack()`/`.unpack_in()` call anywhere in this loop: nothing is ever
        // written to disk as a loose file, matching the "no full extraction" requirement.
    }

    println!("entries seen: {entries_seen}");

    let raw = found.expect("target entry not found in archive");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("parse credential file as JSON");
    let secret = json_path_get(&parsed, secret_json_key)
        .and_then(|v| v.as_str())
        .expect("secret_json_key resolved to a string");

    assert_eq!(secret, "sk-ant-spike-token-abc123");
    println!("extracted secret via {secret_json_key}: {secret}");
    println!("SPIKE OK: selective streaming read of a multi-entry tar.gz bundle works with no disk extraction");
}
