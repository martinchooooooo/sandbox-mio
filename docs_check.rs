//! `./build docs`: checks `docs/` against the project's documentation rules and exits non-zero on
//! any error; `./build docs --fix` first regenerates every `index.md` except the hand-written
//! root `docs/index.md`. This file is the executable form of the rules described in prose in
//! `docs/features/README.md` (frontmatter keys, statuses, log format) and the `write-docs` skill
//! (architecture documents); change the rules here and there together.
//!
//! The base layer is Open Knowledge Format v0.2 (https://okf.md/): every `.md` except `index.md`
//! and `log.md` has YAML frontmatter with a non-empty `type`, `index.md` carries no frontmatter
//! (the root `docs/index.md` may declare `okf_version`), and `log.md` holds dated sections. The
//! project rules on top are listed on [`check`].
//!
//! Compiled into the std-only build tool (`rustc build.rs`), so frontmatter is read by
//! [`parse_frontmatter`], a parser for the restricted YAML subset the docs use rather than a crate.
//! Unit tests: `rustc --test build.rs -o build-test` then run `build-test`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Every allowed `type` value. Mirrored in the frontmatter-keys table of `docs/features/README.md`.
pub const TYPES: &[&str] = &[
    "feature", "ideas", "dev-plan", "e2e-plan", "improvements", "review", "research", "handover",
    "spike", "workflow", "reference", "backlog", "architecture",
];

/// Every allowed `feature.md` `status`, in the order `docs/features/index.md` lists its sections.
pub const STATUSES: &[&str] =
    &["in-progress", "planned", "idea", "implemented", "superseded", "abandoned"];

/// Statuses whose plans are history: their citations of since-moved files are not reported.
const HISTORY_STATUSES: &[&str] = &["implemented", "superseded", "abandoned"];

/// Statuses listed as in-flight plans in the generated `docs/architecture/index.md`.
const IN_FLIGHT_STATUSES: &[&str] = &["in-progress", "planned", "idea"];

/// Bold verbs a `log.md` entry may start with: the log records feature status changes only.
pub const LOG_VERBS: &[&str] = &["Create", "Implement", "Supersede", "Abandon"];

/// Prose words (fenced blocks excluded) above which an architecture document draws a warning.
/// Architecture documents stay a high-level picture; detail belongs in code comments or plans.
pub const ARCHITECTURE_WORD_BUDGET: usize = 700;

/// Repository-relative directories and files scanned for `docs/...` citations outside `docs/`.
const CITATION_ROOTS: &[&str] = &[
    "src", "src-tauri/src", "src-tauri/resources", "e2e", ".claude/skills",
    "AGENTS.md", "CLAUDE.md", "README.md", "src-tauri/AGENTS.md", "build.rs",
];

/// File extensions the citation scan reads.
const CITATION_EXTENSIONS: &[&str] = &["rs", "ts", "js", "svelte", "py", "md", "sh", "ps1"];

/// Directory names the citation scan never descends into.
const CITATION_SKIP_DIRS: &[&str] = &["node_modules", "target", ".diag", ".svelte-kit", "paraglide", "binaries"];

/// One finding. `path` is relative to the repository root.
pub struct Finding {
    pub path: String,
    pub message: String,
}

#[derive(Default)]
pub struct Report {
    pub errors: Vec<Finding>,
    pub warnings: Vec<Finding>,
}

impl Report {
    fn error(&mut self, path: &str, message: String) {
        self.errors.push(Finding { path: path.to_string(), message });
    }
    fn warn(&mut self, path: &str, message: String) {
        self.warnings.push(Finding { path: path.to_string(), message });
    }
}

/// Entry point for `./build docs`; with `fix`, regenerates the `index.md` files first. Returns the
/// process exit code.
pub fn run(root: &Path, fix: bool) -> i32 {
    if fix {
        match write_indexes(root) {
            Ok(written) => {
                for path in &written {
                    println!("wrote {path}");
                }
                println!("docs fix: {} index.md file(s) rewritten", written.len());
            }
            Err(e) => {
                println!("error: docs fix failed: {e}");
                return 1;
            }
        }
    }
    let report = check(root, &today_utc());
    for w in &report.warnings {
        println!("warning: {}: {}", w.path, w.message);
    }
    for e in &report.errors {
        println!("error: {}: {}", e.path, e.message);
    }
    println!(
        "docs check: {} error(s), {} warning(s)",
        report.errors.len(),
        report.warnings.len()
    );
    if report.errors.is_empty() { 0 } else { 1 }
}

// ---------------------------------------------------------------------------------------------
// Frontmatter: a parser for the YAML subset the docs use.
// ---------------------------------------------------------------------------------------------

/// A frontmatter value: a scalar, a list, or a mapping (insertion-ordered).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    /// A scalar or a flow list, as a list of strings; anything else yields nothing.
    pub fn strings(&self) -> Vec<String> {
        match self {
            Value::Str(s) if s.is_empty() => Vec::new(),
            Value::Str(s) => vec![s.clone()],
            Value::List(items) => items.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            Value::Map(_) => Vec::new(),
        }
    }
}

/// Splits a document into its frontmatter source (without the `---` fences) and its body.
/// `text` must already be LF-normalised. `None` means the document has no frontmatter block.
pub fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    if let Some(body) = rest.strip_prefix("---\n") {
        return Some(("", body));
    }
    let end = rest.find("\n---\n").or_else(|| {
        if rest.ends_with("\n---") { Some(rest.len() - 4) } else { None }
    })?;
    let body_start = (end + 5).min(rest.len());
    Some((&rest[..end], &rest[body_start..]))
}

/// Parses the restricted YAML subset: `key: scalar`, `key: [a, b]`, double- or single-quoted
/// scalars, and nested block mappings and `- ` lists by indentation. Comments and blank lines are
/// skipped. Anything else is an error rather than a guess.
pub fn parse_frontmatter(src: &str) -> Result<Value, String> {
    let lines: Vec<(usize, &str)> = src
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| (l.len() - l.trim_start().len(), l.trim_end()))
        .map(|(indent, l)| (indent, &l[indent..]))
        .collect();
    if lines.is_empty() {
        return Ok(Value::Map(Vec::new()));
    }
    if lines[0].0 != 0 {
        return Err("first frontmatter line is indented".to_string());
    }
    parse_block(&lines)
}

fn parse_block(lines: &[(usize, &str)]) -> Result<Value, String> {
    let base = lines[0].0;
    if is_list_item(lines[0].1) {
        let mut items = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let (indent, text) = lines[i];
            if indent != base || !is_list_item(text) {
                return Err(format!("expected a `- ` list item at indent {base}: `{text}`"));
            }
            let after = text[1..].trim_start();
            let mut j = i + 1;
            while j < lines.len() && lines[j].0 > base {
                j += 1;
            }
            if after.is_empty() {
                if j == i + 1 {
                    return Err("empty list item".to_string());
                }
                items.push(parse_block(&lines[i + 1..j])?);
            } else if split_key(after).is_some() && !after.starts_with('"') && !after.starts_with('[') {
                // A mapping item: its first key sits on the dash line, the rest are indented below.
                let mut nested = vec![(base + 2, after)];
                nested.extend_from_slice(&lines[i + 1..j]);
                items.push(parse_block(&nested)?);
            } else if j > i + 1 {
                return Err(format!("scalar list item `{after}` has indented children"));
            } else {
                items.push(parse_scalar(after)?);
            }
            i = j;
        }
        return Ok(Value::List(items));
    }
    let mut entries: Vec<(String, Value)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (indent, text) = lines[i];
        if indent != base {
            return Err(format!("unexpected indentation at `{text}`"));
        }
        let (key, rest) = split_key(text).ok_or_else(|| format!("expected `key: value`, got `{text}`"))?;
        if entries.iter().any(|(k, _)| k == key) {
            return Err(format!("duplicate key `{key}`"));
        }
        let mut j = i + 1;
        while j < lines.len() && lines[j].0 > base {
            j += 1;
        }
        let value = if rest.is_empty() {
            if j == i + 1 { Value::Str(String::new()) } else { parse_block(&lines[i + 1..j])? }
        } else if j > i + 1 {
            return Err(format!("key `{key}` has both an inline value and indented children"));
        } else {
            parse_scalar(rest)?
        };
        entries.push((key.to_string(), value));
        i = j;
    }
    Ok(Value::Map(entries))
}

fn is_list_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

/// Splits `key: rest` at the first `:` that is followed by a space or ends the line. Keys are
/// identifier-like; a line whose prefix is not one is not a key line.
fn split_key(text: &str) -> Option<(&str, &str)> {
    let colon = text.find(':')?;
    let key = &text[..colon];
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    let rest = &text[colon + 1..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some((key, rest.trim()))
}

fn parse_scalar(raw: &str) -> Result<Value, String> {
    let raw = raw.trim();
    if raw.starts_with('[') {
        let inner = raw
            .strip_prefix('[')
            .and_then(|r| r.strip_suffix(']'))
            .ok_or_else(|| format!("unterminated flow list `{raw}`"))?;
        if inner.trim().is_empty() {
            return Ok(Value::List(Vec::new()));
        }
        return inner.split(',').map(|item| parse_scalar(item)).collect::<Result<Vec<_>, _>>().map(Value::List);
    }
    if raw.starts_with('"') {
        let inner = raw
            .strip_prefix('"')
            .and_then(|r| r.strip_suffix('"'))
            .ok_or_else(|| format!("unterminated double-quoted scalar `{raw}`"))?;
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => return Err(format!("dangling escape in `{raw}`")),
                }
            } else if c == '"' {
                return Err(format!("unescaped `\"` inside `{raw}`"));
            } else {
                out.push(c);
            }
        }
        return Ok(Value::Str(out));
    }
    if raw.starts_with('\'') {
        let inner = raw
            .strip_prefix('\'')
            .and_then(|r| r.strip_suffix('\''))
            .ok_or_else(|| format!("unterminated single-quoted scalar `{raw}`"))?;
        return Ok(Value::Str(inner.replace("''", "'")));
    }
    Ok(Value::Str(raw.to_string()))
}

/// ISO 8601 date-time with seconds and an explicit offset (`Z` or `±HH:MM`), as OKF v0.2 requires.
pub fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 20 || !is_date(&s[..10]) || b[10] != b'T' {
        return false;
    }
    let time_ok = digits(&b[11..13]) && b[13] == b':' && digits(&b[14..16]) && b[16] == b':' && digits(&b[17..19]);
    if !time_ok {
        return false;
    }
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let n = frac.bytes().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 {
            return false;
        }
        rest = &frac[n..];
    }
    if rest == "Z" {
        return true;
    }
    let r = rest.as_bytes();
    r.len() == 6 && (r[0] == b'+' || r[0] == b'-') && digits(&r[1..3]) && r[3] == b':' && digits(&r[4..6])
}

/// `YYYY-MM-DD` with plausible month and day.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !digits(&b[..4]) || !digits(&b[5..7]) || !digits(&b[8..10]) {
        return false;
    }
    let month: u32 = s[5..7].parse().unwrap_or(0);
    let day: u32 = s[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn digits(b: &[u8]) -> bool {
    !b.is_empty() && b.iter().all(u8::is_ascii_digit)
}

/// Today's UTC date as `YYYY-MM-DD` (civil-from-days, so no date crate is needed).
pub fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}")
}

// ---------------------------------------------------------------------------------------------
// Markdown helpers.
// ---------------------------------------------------------------------------------------------

/// Removes fenced code blocks and inline code spans, so example links inside code are not checked.
pub fn strip_code(body: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push('\n');
            continue;
        }
        if in_fence {
            out.push('\n');
            continue;
        }
        let mut in_span = false;
        for c in line.chars() {
            if c == '`' {
                in_span = !in_span;
            } else if !in_span {
                out.push(c);
            }
        }
        out.push('\n');
    }
    out
}

/// Relative link targets of `[text](target)` links, without `#fragment`. External URLs, pure
/// fragments and `mailto:` links are skipped. Code is stripped first.
pub fn relative_links(body: &str) -> Vec<String> {
    let text = strip_code(body);
    let mut links = Vec::new();
    let mut rest = text.as_str();
    while let Some(pos) = rest.find("](") {
        rest = &rest[pos + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = rest[..end].trim();
        rest = &rest[end..];
        let target = target.split('#').next().unwrap_or("");
        if target.is_empty() || target.contains("://") || target.starts_with("mailto:") || target.contains(' ') {
            continue;
        }
        links.push(target.to_string());
    }
    links
}

/// `docs/...md` paths cited inside inline code spans.
fn cited_doc_paths(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, span) in body.split('`').enumerate() {
        let placeholder = span.contains('*') || span.contains('<');
        if i % 2 == 1 && span.starts_with("docs/") && span.ends_with(".md") && !span.contains(' ') && !placeholder {
            out.push(span.to_string());
        }
    }
    out
}

/// Lexically resolves `target` against `dir` (no filesystem access), collapsing `.` and `..`.
fn resolve(dir: &Path, target: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for c in dir.join(target).components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `docs/...` paths cited in source code or prose outside `docs/`: `.md` files and directories
/// written with a trailing `/`. A path ends at the first character a path cannot contain, so a
/// `<placeholder>` segment cuts it short; a URL's `docs/` follows a `/` and is skipped.
pub fn code_citations(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(pos) = text[from..].find("docs/") {
        let start = from + pos;
        from = start + "docs/".len();
        if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || b"/._-\\".contains(&bytes[start - 1])) {
            continue;
        }
        let end = text[start..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-')))
            .map_or(text.len(), |n| start + n);
        let path = text[start..end].trim_end_matches('.');
        if path.ends_with(".md") || (path.ends_with('/') && path.len() > "docs/".len()) {
            out.push(path.to_string());
        }
    }
    out
}

/// Words of prose in a markdown body; fenced blocks (diagrams, examples) do not count.
pub fn prose_words(body: &str) -> usize {
    let mut in_fence = false;
    let mut words = 0;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence {
            words += line.split_whitespace().count();
        }
    }
    words
}

/// An OKF actor: `<producer>/<version>`, `human:<id>` or `process:<id>`.
pub fn is_actor(s: &str) -> bool {
    if s.contains(char::is_whitespace) {
        return false;
    }
    for prefix in ["human:", "process:"] {
        if let Some(id) = s.strip_prefix(prefix) {
            return !id.is_empty();
        }
    }
    matches!(s.split_once('/'), Some((producer, version)) if !producer.is_empty() && !version.is_empty())
}

// ---------------------------------------------------------------------------------------------
// The checks.
// ---------------------------------------------------------------------------------------------

struct Doc {
    /// Path relative to the repository root, `/`-separated, for messages and lookups.
    rel: String,
    abs: PathBuf,
    body: String,
    /// Whether the file opens with a `---` frontmatter block, parsed or not.
    has_block: bool,
    /// `Some` when the document has a frontmatter block that parsed.
    fm: Option<Value>,
}

/// What a feature's `feature.md` declares, keyed by feature directory name in [`features`].
struct FeatureInfo {
    status: String,
    description: String,
    architecture: Vec<String>,
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn file_name(p: &Path) -> &str {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("")
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    paths
}

fn is_markdown(p: &Path) -> bool {
    p.is_file() && p.extension().map_or(false, |e| e == "md")
}

/// Whether `dir` holds a document (any `.md` other than `index.md`) anywhere beneath it.
fn has_markdown(dir: &Path) -> bool {
    sorted_entries(dir)
        .iter()
        .any(|p| if p.is_dir() { has_markdown(p) } else { is_markdown(p) && file_name(p) != "index.md" })
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    for p in sorted_entries(dir) {
        if p.is_dir() {
            collect_markdown(&p, out);
        } else if is_markdown(&p) {
            out.push(p);
        }
    }
}

/// Reads and parses every markdown document under `docs/`, reporting unreadable files and
/// frontmatter that does not parse.
fn load(root: &Path, report: &mut Report) -> Vec<Doc> {
    let mut paths = Vec::new();
    collect_markdown(&root.join("docs"), &mut paths);
    let mut docs = Vec::new();
    for abs in paths {
        let rel = rel_of(root, &abs);
        let text = match fs::read(&abs) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).replace("\r\n", "\n"),
            Err(e) => {
                report.error(&rel, format!("unreadable: {e}"));
                continue;
            }
        };
        let (has_block, fm, body) = match split_frontmatter(&text) {
            Some((src, body)) => match parse_frontmatter(src) {
                Ok(v) => (true, Some(v), body.to_string()),
                Err(e) => {
                    report.error(&rel, format!("frontmatter does not parse: {e}"));
                    (true, None, body.to_string())
                }
            },
            None => (false, None, text.clone()),
        };
        docs.push(Doc { rel, abs, body, has_block, fm });
    }
    docs
}

/// Every `docs/features/<f>/feature.md` that parsed, keyed by `<f>`.
fn features(docs: &[Doc]) -> BTreeMap<String, FeatureInfo> {
    let mut out = BTreeMap::new();
    for doc in docs {
        let Some(name) = doc.rel.strip_prefix("docs/features/").and_then(|r| r.strip_suffix("/feature.md")) else {
            continue;
        };
        let Some(fm) = doc.fm.as_ref().filter(|_| !name.contains('/')) else { continue };
        let text = |key: &str| fm.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        out.insert(
            name.to_string(),
            FeatureInfo {
                status: text("status"),
                description: text("description"),
                architecture: fm.get("architecture").map(Value::strings).unwrap_or_default(),
            },
        );
    }
    out
}

/// Whether `rel` is a `log.md` or a document of a feature whose status makes it history.
fn is_history(rel: &str, feats: &BTreeMap<String, FeatureInfo>) -> bool {
    if rel.ends_with("/log.md") {
        return true;
    }
    let Some((name, _)) = rel.strip_prefix("docs/features/").and_then(|r| r.split_once('/')) else {
        return false;
    };
    feats.get(name).map_or(false, |f| HISTORY_STATUSES.contains(&f.status.as_str()))
}

/// Checks the `docs/` tree under `root` (the repository root). `today` is `YYYY-MM-DD`.
///
/// Errors (exit code 1):
/// - OKF base: frontmatter on every non-reserved `.md` with a `type` from [`TYPES`]; no frontmatter
///   on `index.md` except `okf_version` on `docs/index.md`; `log.md` sections dated `## YYYY-MM-DD`,
///   newest first, entries starting with a bold verb from [`LOG_VERBS`].
/// - Every frontmatter document has a `title`; a `timestamp`, where present, has a UTC offset.
/// - Every relative markdown link resolves, and every document is linked from some `index.md`.
/// - `type: architecture` only under `docs/architecture/`, with `description`, `stale_after`,
///   `generated` (actor `by`, timestamp `at`), well-formed `verified` entries if any, non-empty
///   `sources` and `code` lists of existing repository-relative paths, and no gotchas section.
/// - Each `docs/features/<f>/` has `feature.md` with `type: feature`, `title` equal to the
///   directory name, a `description`, a `status` from [`STATUSES`], existing
///   `depends_on`/`superseded_by` features, `superseded_by` when superseded, and existing
///   `architecture` paths. Other documents in the directory carry `feature: <f>`.
/// - Every generated `index.md` matches what `./build docs --fix` would write.
///
/// Warnings: an architecture document past its `stale_after` or over
/// [`ARCHITECTURE_WORD_BUDGET`] words, and a cited `docs/...` path that does not exist, whether
/// cited from a document that is not history or from code (see [`CITATION_ROOTS`]).
pub fn check(root: &Path, today: &str) -> Report {
    let mut report = Report::default();
    let docs = load(root, &mut report);
    if docs.is_empty() {
        report.error("docs", "no markdown documents found".to_string());
        return report;
    }
    for doc in &docs {
        match file_name(&doc.abs) {
            "index.md" => check_index_frontmatter(&mut report, &doc.rel, doc.has_block, doc.fm.as_ref()),
            "log.md" => {
                if doc.has_block {
                    report.error(&doc.rel, "log.md must not have frontmatter".to_string());
                }
                check_log(&mut report, &doc.rel, &doc.body);
            }
            _ => {
                if !doc.has_block {
                    report.error(&doc.rel, "missing YAML frontmatter (`---` block before the H1)".to_string());
                } else if let Some(v) = &doc.fm {
                    check_common(&mut report, &doc.rel, v);
                }
            }
        }
    }
    let feats = features(&docs);
    check_links_and_reachability(&mut report, root, &docs, &feats);
    check_architecture(&mut report, root, &docs, today);
    check_features(&mut report, root, &docs);
    check_indexes(&mut report, root, &docs);
    check_code_citations(&mut report, root);
    report
}

fn check_index_frontmatter(report: &mut Report, rel: &str, has_block: bool, fm: Option<&Value>) {
    if !has_block {
        return;
    }
    if rel != "docs/index.md" {
        report.error(rel, "index.md must not have frontmatter".to_string());
        return;
    }
    if let Some(Value::Map(entries)) = fm {
        for (k, _) in entries {
            if k != "okf_version" {
                report.error(rel, format!("root index.md frontmatter may only declare `okf_version`, found `{k}`"));
            }
        }
    }
}

fn check_common(report: &mut Report, rel: &str, fm: &Value) {
    match fm.get("type").and_then(Value::as_str) {
        None | Some("") => report.error(rel, "frontmatter is missing a non-empty `type`".to_string()),
        Some(t) if !TYPES.contains(&t) => {
            report.error(rel, format!("unknown type `{t}` (allowed: {})", TYPES.join(", ")))
        }
        _ => {}
    }
    if fm.get("title").and_then(Value::as_str).map_or(true, str::is_empty) {
        report.error(rel, "frontmatter is missing `title`".to_string());
    }
    if let Some(ts) = fm.get("timestamp").and_then(Value::as_str) {
        if !is_timestamp(ts) {
            report.error(
                rel,
                format!("`timestamp: {ts}` is not ISO 8601 with seconds and a UTC offset (e.g. 2026-11-05T23:10:11+11:00)"),
            );
        }
    }
}

fn check_log(report: &mut Report, rel: &str, body: &str) {
    let mut last_date: Option<String> = None;
    let mut seen_heading = false;
    for line in body.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            let h = h.trim();
            if !is_date(h) {
                report.error(rel, format!("log section heading `## {h}` is not `## YYYY-MM-DD`"));
                continue;
            }
            if let Some(prev) = &last_date {
                if h > prev.as_str() {
                    report.error(rel, format!("log section {h} is above older section {prev}; newest first"));
                } else if h == prev.as_str() {
                    report.error(rel, format!("log section {h} appears twice"));
                }
            }
            last_date = Some(h.to_string());
            seen_heading = true;
        } else if line.starts_with("* ") || line.starts_with("- ") {
            if !seen_heading {
                report.error(rel, format!("log entry before the first dated section: `{line}`"));
                continue;
            }
            let verb = line[2..].strip_prefix("**").and_then(|r| r.split("**").next());
            match verb {
                Some(v) if LOG_VERBS.contains(&v) => {}
                _ => report.error(
                    rel,
                    format!("log entry must start with a bold verb ({}): `{line}`", LOG_VERBS.join(", ")),
                ),
            }
        }
    }
    if !seen_heading {
        report.error(rel, "log.md has no `## YYYY-MM-DD` section".to_string());
    }
}

fn check_links_and_reachability(
    report: &mut Report,
    root: &Path,
    docs: &[Doc],
    feats: &BTreeMap<String, FeatureInfo>,
) {
    let mut linked_from_index: BTreeSet<PathBuf> = BTreeSet::new();
    for doc in docs {
        let dir = doc.abs.parent().unwrap_or(root);
        let is_index = file_name(&doc.abs) == "index.md";
        for target in relative_links(&doc.body) {
            let resolved = resolve(dir, &target);
            if !resolved.exists() {
                report.error(&doc.rel, format!("broken link `{target}`"));
                continue;
            }
            if is_index {
                let file = if resolved.is_dir() { resolved.join("index.md") } else { resolved.clone() };
                linked_from_index.insert(file);
            }
        }
        if is_history(&doc.rel, feats) {
            continue;
        }
        let mut seen = BTreeSet::new();
        for cited in cited_doc_paths(&doc.body) {
            if seen.insert(cited.clone()) && !root.join(&cited).exists() {
                report.warn(&doc.rel, format!("cites missing `{cited}`"));
            }
        }
    }
    let root_index = resolve(root, "docs/index.md");
    for doc in docs {
        let abs = resolve(root, &doc.rel);
        if abs != root_index && !linked_from_index.contains(&abs) {
            report.error(&doc.rel, "not linked from any index.md".to_string());
        }
    }
}

/// Checks that `key` is a non-empty list of repository-relative paths that exist, read from each
/// item directly or, with `item_key`, from that key of each mapping item.
fn check_path_list(report: &mut Report, rel: &str, root: &Path, fm: &Value, key: &str, item_key: Option<&str>) {
    let Some(Value::List(items)) = fm.get(key).filter(|v| matches!(v, Value::List(items) if !items.is_empty())) else {
        report.error(rel, format!("architecture document needs a non-empty `{key}` list"));
        return;
    };
    for item in items {
        let path = match item_key {
            Some(k) => item.get(k).and_then(Value::as_str),
            None => item.as_str(),
        };
        match path {
            Some(p) if p.split('/').any(|seg| seg == "..") => {
                report.error(rel, format!("`{key}` path `{p}` must be repository-relative, without `..`"))
            }
            Some(p) if root.join(p).exists() => {}
            Some(p) => report.error(rel, format!("`{key}` path `{p}` does not exist (paths are relative to the repository root)")),
            None => report.error(rel, format!("every `{key}` item needs a path{}", item_key.map(|k| format!(" in `{k}`")).unwrap_or_default())),
        }
    }
}

fn check_architecture(report: &mut Report, root: &Path, docs: &[Doc], today: &str) {
    for doc in docs {
        let Some(fm) = &doc.fm else { continue };
        let rel = doc.rel.as_str();
        let ty = fm.get("type").and_then(Value::as_str).unwrap_or("");
        let in_arch = rel.starts_with("docs/architecture/");
        if ty == "architecture" && !in_arch {
            report.error(rel, "`type: architecture` belongs only under docs/architecture/".to_string());
        }
        if in_arch && ty != "architecture" {
            report.error(rel, "documents under docs/architecture/ must be `type: architecture`".to_string());
        }
        if ty != "architecture" {
            continue;
        }
        if fm.get("description").and_then(Value::as_str).map_or(true, str::is_empty) {
            report.error(rel, "architecture document is missing a one-sentence `description`".to_string());
        }
        match fm.get("stale_after").and_then(Value::as_str) {
            None => report.error(rel, "architecture document is missing `stale_after`".to_string()),
            Some(s) if !is_timestamp(s) => report.error(rel, format!("`stale_after: {s}` is not a timestamp")),
            Some(s) if &s[..10] < today => report.warn(
                rel,
                format!("stale since {}: re-verify against the code, then move `stale_after` forward", &s[..10]),
            ),
            _ => {}
        }
        let generated = fm.get("generated");
        if !generated.and_then(|g| g.get("by")).and_then(Value::as_str).map_or(false, is_actor) {
            report.error(rel, "architecture document needs `generated.by` as an actor (`<producer>/<version>` or `human:<id>`)".to_string());
        }
        if !generated.and_then(|g| g.get("at")).and_then(Value::as_str).map_or(false, is_timestamp) {
            report.error(rel, "architecture document is missing a valid `generated.at`".to_string());
        }
        if let Some(verified) = fm.get("verified") {
            let entries: Vec<&Value> = match verified {
                Value::List(items) => items.iter().collect(),
                Value::Map(_) => vec![verified],
                Value::Str(_) => Vec::new(),
            };
            let well_formed = |e: &&Value| {
                e.get("by").and_then(Value::as_str).map_or(false, is_actor)
                    && e.get("at").and_then(Value::as_str).map_or(false, is_timestamp)
            };
            if entries.is_empty() || !entries.iter().all(well_formed) {
                report.error(rel, "every `verified` entry needs an actor `by` (e.g. `human:<id>`) and a timestamp `at`".to_string());
            }
        }
        check_path_list(report, rel, root, fm, "sources", Some("resource"));
        check_path_list(report, rel, root, fm, "code", None);
        if doc.body.lines().any(|l| l.starts_with('#') && l.to_ascii_lowercase().contains("gotcha")) {
            report.error(rel, "architecture documents carry no gotchas section; state rules as Invariants, and put code-level gotchas in code comments or the area's AGENTS.md".to_string());
        }
        let words = prose_words(&doc.body);
        if words > ARCHITECTURE_WORD_BUDGET {
            report.warn(rel, format!("{words} words is over the {ARCHITECTURE_WORD_BUDGET}-word budget; move detail to code comments or plans"));
        }
    }
}

fn check_features(report: &mut Report, root: &Path, docs: &[Doc]) {
    let features_dir = root.join("docs").join("features");
    let feature_names: Vec<String> = sorted_entries(&features_dir)
        .iter()
        .filter(|p| p.is_dir())
        .map(|p| file_name(p).to_string())
        .collect();
    let known: BTreeSet<&str> = feature_names.iter().map(String::as_str).collect();

    for name in &feature_names {
        let feature_rel = format!("docs/features/{name}/feature.md");
        let Some(doc) = docs.iter().find(|d| d.rel == feature_rel) else {
            report.error(&format!("docs/features/{name}"), "feature directory has no feature.md".to_string());
            continue;
        };
        let Some(fm) = &doc.fm else { continue };
        let rel = &doc.rel;
        if fm.get("type").and_then(Value::as_str) != Some("feature") {
            report.error(rel, "feature.md must be `type: feature`".to_string());
        }
        if fm.get("title").and_then(Value::as_str) != Some(name.as_str()) {
            report.error(rel, format!("feature.md `title` must equal the directory name `{name}`"));
        }
        if fm.get("description").and_then(Value::as_str).map_or(true, str::is_empty) {
            report.error(rel, "feature.md is missing a one-line `description`".to_string());
        }
        let status = fm.get("status").and_then(Value::as_str).unwrap_or("");
        if !STATUSES.contains(&status) {
            report.error(rel, format!("`status: {status}` is not one of {}", STATUSES.join(", ")));
        }
        for key in ["depends_on", "superseded_by"] {
            for dep in fm.get(key).map(Value::strings).unwrap_or_default() {
                if !known.contains(dep.as_str()) {
                    report.error(rel, format!("`{key}` names unknown feature `{dep}`"));
                }
            }
        }
        if status == "superseded" && fm.get("superseded_by").map(Value::strings).unwrap_or_default().is_empty() {
            report.error(rel, "`status: superseded` requires `superseded_by`".to_string());
        }
        for path in fm.get("architecture").map(Value::strings).unwrap_or_default() {
            if !root.join(&path).exists() {
                report.error(rel, format!("`architecture` path `{path}` does not exist"));
            } else if !path.starts_with("docs/architecture/") {
                report.error(rel, format!("`architecture` path `{path}` is not under docs/architecture/"));
            }
        }
    }

    // Documents inside a feature directory carry `feature: <dir>`.
    for doc in docs {
        let Some(rest) = doc.rel.strip_prefix("docs/features/") else { continue };
        let Some((feature, file)) = rest.split_once('/') else { continue };
        let fname = file.rsplit('/').next().unwrap_or(file);
        if fname == "index.md" || fname == "log.md" || file == "feature.md" {
            continue;
        }
        let Some(fm) = &doc.fm else { continue };
        if fm.get("feature").and_then(Value::as_str) != Some(feature) {
            report.error(&doc.rel, format!("document inside a feature directory needs `feature: {feature}`"));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Generated indexes.
// ---------------------------------------------------------------------------------------------

/// The index description of a document that has no `description`, from its `type`.
fn type_phrase(ty: &str) -> &'static str {
    match ty {
        "ideas" => "ideation and scope",
        "dev-plan" => "build plan: phases and resolved decisions",
        "e2e-plan" => "end-to-end test plan",
        "improvements" => "deferred work and known limitations",
        "review" => "post-implementation review findings",
        "research" => "research notes",
        "handover" => "handover notes",
        "spike" => "spike record",
        "workflow" => "workflow and conventions",
        "reference" => "reference",
        "backlog" => "backlog",
        _ => "",
    }
}

fn file_line(fname: &str, doc: Option<&Doc>) -> String {
    if fname == "log.md" {
        return "* [Log](log.md) - dated record of feature status changes".to_string();
    }
    let fm = doc.and_then(|d| d.fm.as_ref());
    let get = |key: &str| fm.and_then(|v| v.get(key)).and_then(Value::as_str).filter(|s| !s.is_empty());
    let title = get("title").unwrap_or(fname.trim_end_matches(".md"));
    let desc = get("description").unwrap_or_else(|| type_phrase(get("type").unwrap_or("")));
    if desc.is_empty() { format!("* [{title}]({fname})") } else { format!("* [{title}]({fname}) - {desc}") }
}

fn subdir_line(name: &str, has_md: bool) -> String {
    let spike = name == "spike" || name == "spikes";
    match (has_md, spike) {
        (true, true) => format!("* [{name}/]({name}/index.md) - spike records and scripts"),
        (true, false) => format!("* [{name}/]({name}/index.md)"),
        (false, true) => format!("* [{name}/]({name}/) - spike scripts (no markdown)"),
        (false, false) => format!("* [{name}/]({name}/) - files (no markdown)"),
    }
}

/// Builds the index of `dir` and, recursively, of every subdirectory holding documents.
fn index_tree(
    root: &Path,
    dir: &Path,
    by_rel: &BTreeMap<&str, &Doc>,
    feats: &BTreeMap<String, FeatureInfo>,
    out: &mut BTreeMap<String, String>,
) {
    if !has_markdown(dir) {
        return;
    }
    let rel = rel_of(root, dir);
    let entries = sorted_entries(dir);
    let is_feature_dir = rel.rsplit_once('/').map_or(false, |(parent, _)| parent == "docs/features");
    let mut lines: Vec<String> = Vec::new();
    if is_feature_dir {
        if let Some(f) = feats.get(file_name(dir)) {
            lines.push(format!("* [Feature status](feature.md) - {}: {}", f.status, f.description));
        }
    }
    let mut files: Vec<&PathBuf> = entries
        .iter()
        .filter(|p| is_markdown(p) && file_name(p) != "index.md" && !(is_feature_dir && file_name(p) == "feature.md"))
        .collect();
    if rel == "docs/architecture" {
        files.sort_by_key(|p| file_name(p) != "overview.md");
    }
    for p in files {
        lines.push(file_line(file_name(p), by_rel.get(rel_of(root, p).as_str()).copied()));
    }
    if rel == "docs/features" {
        for status in STATUSES {
            let listed: Vec<(&String, &FeatureInfo)> = feats.iter().filter(|(_, f)| f.status == *status).collect();
            if listed.is_empty() {
                continue;
            }
            lines.extend([String::new(), format!("## {status}"), String::new()]);
            for (name, f) in listed {
                lines.push(format!("* [{name}]({name}/index.md) - {}", f.description));
            }
        }
    } else {
        for p in entries.iter().filter(|p| p.is_dir()) {
            lines.push(subdir_line(file_name(p), has_markdown(p)));
        }
    }
    if rel == "docs/architecture" {
        let in_flight: Vec<(&String, &FeatureInfo)> = IN_FLIGHT_STATUSES
            .iter()
            .flat_map(|s| feats.iter().filter(move |(_, f)| f.status == *s && !f.architecture.is_empty()))
            .collect();
        if !in_flight.is_empty() {
            lines.extend([String::new(), "## In-flight plans".to_string(), String::new()]);
            for (name, f) in in_flight {
                let areas: Vec<&str> =
                    f.architecture.iter().map(|a| a.strip_prefix("docs/architecture/").unwrap_or(a)).collect();
                lines.push(format!(
                    "* [{name}](../features/{name}/index.md) - {}, changes {}: {}",
                    f.status,
                    areas.join(", "),
                    f.description
                ));
            }
        }
    }
    out.insert(format!("{rel}/index.md"), lines.join("\n") + "\n");
    for p in entries.iter().filter(|p| p.is_dir()) {
        index_tree(root, p, by_rel, feats, out);
    }
}

/// The generated content of every `index.md` under `docs/` except the hand-written root
/// `docs/index.md`, keyed by repository-relative path.
fn expected_indexes(root: &Path, docs: &[Doc]) -> BTreeMap<String, String> {
    let by_rel: BTreeMap<&str, &Doc> = docs.iter().map(|d| (d.rel.as_str(), d)).collect();
    let feats = features(docs);
    let mut out = BTreeMap::new();
    for p in sorted_entries(&root.join("docs")).iter().filter(|p| p.is_dir()) {
        index_tree(root, p, &by_rel, &feats, &mut out);
    }
    out
}

fn read_normalised(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).replace("\r\n", "\n"))
}

fn check_indexes(report: &mut Report, root: &Path, docs: &[Doc]) {
    for (rel, expected) in expected_indexes(root, docs) {
        match read_normalised(&root.join(&rel)) {
            Some(current) if current == expected => {}
            Some(_) => report.error(&rel, "out of date with the documents it lists; run `./build docs --fix`".to_string()),
            None => report.error(&rel, "missing; run `./build docs --fix`".to_string()),
        }
    }
}

/// Rewrites every generated `index.md` whose content differs, returning the paths written.
pub fn write_indexes(root: &Path) -> Result<Vec<String>, String> {
    let docs = load(root, &mut Report::default());
    let mut written = Vec::new();
    for (rel, expected) in expected_indexes(root, &docs) {
        let path = root.join(&rel);
        if read_normalised(&path).as_deref() != Some(expected.as_str()) {
            fs::write(&path, &expected).map_err(|e| format!("{rel}: {e}"))?;
            written.push(rel);
        }
    }
    Ok(written)
}

// ---------------------------------------------------------------------------------------------
// Citations from code.
// ---------------------------------------------------------------------------------------------

fn collect_citing_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if CITATION_EXTENSIONS.contains(&ext) {
            out.push(path.to_path_buf());
        }
        return;
    }
    for p in sorted_entries(path) {
        if !(p.is_dir() && CITATION_SKIP_DIRS.contains(&file_name(&p))) {
            collect_citing_files(&p, out);
        }
    }
}

fn check_code_citations(report: &mut Report, root: &Path) {
    let mut files = Vec::new();
    for r in CITATION_ROOTS {
        collect_citing_files(&root.join(r), &mut files);
    }
    for path in files {
        let Some(text) = read_normalised(&path) else { continue };
        let rel = rel_of(root, &path);
        let mut seen = BTreeSet::new();
        for cited in code_citations(&text) {
            if seen.insert(cited.clone()) && !root.join(&cited).exists() {
                report.warn(&rel, format!("cites missing `{cited}`"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_frontmatter_shapes_the_docs_use() {
        let src = "type: architecture\ntitle: \"A: B\"\nfeature: x\ndepends_on: [a, b]\n\
                   generated:\n  by: claude-code/opus-5-5\n  at: 2026-11-05T23:10:11+11:00\n\
                   code:\n  - src/a/\n  - src/b.rs\n\
                   sources:\n  - resource: docs/a.md\n  - resource: docs/b.md\n";
        let v = parse_frontmatter(src).unwrap();
        assert_eq!(v.get("title").and_then(Value::as_str), Some("A: B"));
        assert_eq!(v.get("depends_on").map(Value::strings), Some(vec!["a".to_string(), "b".to_string()]));
        assert_eq!(v.get("generated").and_then(|g| g.get("by")).and_then(Value::as_str), Some("claude-code/opus-5-5"));
        assert_eq!(v.get("code").map(Value::strings), Some(vec!["src/a/".to_string(), "src/b.rs".to_string()]));
        match v.get("sources") {
            Some(Value::List(items)) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[1].get("resource").and_then(Value::as_str), Some("docs/b.md"));
            }
            other => panic!("sources parsed as {:?}", other),
        }
    }

    #[test]
    fn code_citations_find_doc_paths_but_not_urls() {
        let text = "// see docs/features/x/02_dev.md, Resolved Decision 2.\n\
                    // `docs/architecture/` and docs/features/<x>/02_dev.md\n\
                    // https://example.com/docs/y.md and mydocs/z.md and docs/notes.txt";
        assert_eq!(
            code_citations(text),
            vec!["docs/features/x/02_dev.md".to_string(), "docs/architecture/".to_string(), "docs/features/".to_string()]
        );
    }

    #[test]
    fn prose_words_skip_fenced_blocks() {
        assert_eq!(prose_words("one two\n```mermaid\nA --> B\n```\nthree\n"), 3);
    }

    #[test]
    fn actors_follow_okf() {
        assert!(is_actor("claude-code/opus-5-5"));
        assert!(is_actor("human:martin"));
        assert!(!is_actor("claude"));
        assert!(!is_actor("human:"));
        assert!(!is_actor("a b/c"));
    }

    #[test]
    fn generated_indexes_pass_the_check() {
        let root = std::env::temp_dir().join(format!("docs-check-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let write = |rel: &str, text: &str| {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        fs::create_dir_all(root.join("src/x")).unwrap();
        write("docs/index.md", "* [architecture/](architecture/index.md) - a\n* [features/](features/index.md) - f\n");
        write("docs/features/README.md", "---\ntype: workflow\ntitle: \"Features\"\ndescription: \"How plans work.\"\n---\n# Features\n");
        write("docs/features/log.md", "## 2026-11-05\n\n* **Create** [x](x/index.md): drafted\n");
        write(
            "docs/features/x/feature.md",
            "---\ntype: feature\ntitle: \"x\"\ndescription: \"Does x.\"\nstatus: planned\narchitecture: [docs/architecture/overview.md]\n---\n# x\n",
        );
        write("docs/features/x/01_ideas.md", "---\ntype: ideas\ntitle: \"X Ideas\"\nfeature: x\n---\n# X Ideas\n");
        write("docs/features/x/spike/run.sh", "echo\n");
        write(
            "docs/architecture/overview.md",
            "---\ntype: architecture\ntitle: \"Overview\"\ndescription: \"The map.\"\n\
             generated:\n  by: claude-code/opus-5-5\n  at: 2026-11-05T23:10:11+11:00\n\
             verified:\n  by: human:martin\n  at: 2026-11-06T09:00:00+11:00\n\
             stale_after: 2099-01-01T00:00:00Z\ncode:\n  - src/x/\nsources:\n  - resource: docs/features/x/01_ideas.md\n---\n# Overview\n",
        );
        let before = check(&root, "2026-11-05");
        assert!(before.errors.iter().any(|e| e.message.contains("--fix")));
        let written = write_indexes(&root).unwrap();
        assert_eq!(written.len(), 3, "{written:?}");
        let report = check(&root, "2026-11-05");
        assert!(report.errors.is_empty(), "{:?}", report.errors.iter().map(|e| format!("{}: {}", e.path, e.message)).collect::<Vec<_>>());
        let feature_index = fs::read_to_string(root.join("docs/features/x/index.md")).unwrap();
        assert_eq!(
            feature_index,
            "* [Feature status](feature.md) - planned: Does x.\n\
             * [X Ideas](01_ideas.md) - ideation and scope\n\
             * [spike/](spike/) - spike scripts (no markdown)\n"
        );
        let arch_index = fs::read_to_string(root.join("docs/architecture/index.md")).unwrap();
        assert!(arch_index.contains("* [x](../features/x/index.md) - planned, changes overview.md: Does x."), "{}", arch_index);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_malformed_frontmatter() {
        assert!(parse_frontmatter("type feature\n").is_err());
        assert!(parse_frontmatter("type: a\ntype: b\n").is_err());
        assert!(parse_frontmatter("title: \"unterminated\n").is_err());
        assert!(parse_frontmatter("  type: indented\n").is_err());
    }

    #[test]
    fn splits_frontmatter_from_body() {
        let (fm, body) = split_frontmatter("---\ntype: x\n---\n# T\n").unwrap();
        assert_eq!(fm, "type: x");
        assert_eq!(body, "# T\n");
        assert!(split_frontmatter("# No frontmatter\n").is_none());
    }

    #[test]
    fn timestamps_need_an_offset() {
        assert!(is_timestamp("2026-11-05T23:10:11+11:00"));
        assert!(is_timestamp("2026-11-05T12:10:11Z"));
        assert!(is_timestamp("2026-11-05T12:10:11.5Z"));
        assert!(!is_timestamp("2026-11-05T23:10:11"));
        assert!(!is_timestamp("2026-11-05"));
        assert!(!is_timestamp("2026-13-05T23:10:11Z"));
    }

    #[test]
    fn links_inside_code_are_ignored() {
        let body = "See [a](a.md) and `[b](b.md)` and [c](https://x.y/c.md) and [d](d.md#frag).\n```\n[e](e.md)\n```\n";
        assert_eq!(relative_links(body), vec!["a.md".to_string(), "d.md".to_string()]);
    }

    #[test]
    fn log_rules() {
        let mut r = Report::default();
        check_log(&mut r, "log.md", "# Log\n\n## 2026-11-05\n\n* **Create** [x](x/index.md): y\n\n## 2026-11-04\n\n* **Implement** z\n");
        assert!(r.errors.is_empty(), "{:?}", r.errors.iter().map(|e| &e.message).collect::<Vec<_>>());
        let mut r = Report::default();
        check_log(&mut r, "log.md", "## 2026-11-04\n\n* **Create** a\n\n## 2026-11-05\n\n* Did a thing\n");
        assert_eq!(r.errors.len(), 2);
    }

    #[test]
    fn today_is_a_date() {
        assert!(is_date(&today_utc()));
    }
}
