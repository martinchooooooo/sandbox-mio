//! `cargo xtask docs`: checks `docs/` against the project's documentation rules and exits non-zero on
//! any error; `cargo xtask docs --fix` first regenerates every `index.md` except the hand-written
//! root `docs/index.md`. This file is the executable form of the rules described in prose in
//! `docs/features/README.md` (frontmatter keys, statuses, log format) and the `write-docs` skill
//! (architecture documents); change the rules here and there together.
//!
//! The base layer is Open Knowledge Format v0.2 (https://okf.md/): every `.md` except `index.md`
//! and `log.md` has YAML frontmatter with a non-empty `type`, `index.md` carries no frontmatter
//! (the root `docs/index.md` may declare `okf_version`), and `log.md` holds dated sections. The
//! project rules on top are listed on [`check`].
//!
//! Frontmatter is parsed by `yaml-rust2` and Markdown by `pulldown-cmark`; both are converted at the
//! edge ([`parse_frontmatter`], [`markdown_facts`]) so the checks below never see a crate type.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
use yaml_rust2::parser::{Event as YamlEvent, Parser as YamlParser};

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
pub const ARCHITECTURE_WORD_BUDGET: usize = 750;

/// Repository-relative directories and files scanned for `docs/...` citations outside `docs/`.
const CITATION_ROOTS: &[&str] = &[
    "src", "src-tauri/src", "src-tauri/crates", "src-tauri/resources", "e2e", ".claude/skills",
    "AGENTS.md", "CLAUDE.md", "README.md", "src-tauri/AGENTS.md", "xtask/src",
];

/// File names the citation scan skips: this file's comments quote `docs/...` patterns that are not
/// paths.
const CITATION_SKIP_FILES: &[&str] = &["docs_check.rs"];

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

/// Entry point for `cargo xtask docs`; with `fix`, regenerates the `index.md` files first. Returns the
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
// Frontmatter: YAML read by yaml-rust2 into a small value tree the checks walk.
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
/// A leading UTF-8 BOM and trailing whitespace after a `---` fence are tolerated.
pub fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let start = first.len();
    let mut pos = start;
    for line in lines {
        if line.trim_end() == "---" {
            let source = text[start..pos].strip_suffix('\n').unwrap_or(&text[start..pos]);
            return Some((source, &text[pos + line.len()..]));
        }
        pos += line.len();
    }
    None
}

/// Parses frontmatter YAML into a [`Value`]: a mapping of scalars, lists and nested mappings.
///
/// Every scalar keeps its source text (`007`, `yes` and `1.0` stay strings), so the checks see what
/// is written. Duplicate keys, aliases, non-scalar keys and a second document are errors, and the
/// top level must be a mapping (an empty source is an empty mapping).
pub fn parse_frontmatter(src: &str) -> Result<Value, String> {
    let mut stack: Vec<Frame> = Vec::new();
    let mut root: Option<Value> = None;
    let mut documents = 0;
    let mut parser = YamlParser::new_from_str(src);
    loop {
        let (event, mark) = parser.next_token().map_err(|e| e.to_string())?;
        if matches!(event, YamlEvent::StreamEnd) {
            break;
        }
        let finished = match event {
            YamlEvent::DocumentStart => {
                documents += 1;
                if documents > 1 {
                    return Err("more than one YAML document".to_string());
                }
                None
            }
            YamlEvent::Scalar(text, ..) => Some(Value::Str(text)),
            YamlEvent::SequenceStart(..) => {
                stack.push(Frame::List(Vec::new()));
                None
            }
            YamlEvent::MappingStart(..) => {
                stack.push(Frame::Map { entries: Vec::new(), key: None });
                None
            }
            YamlEvent::SequenceEnd | YamlEvent::MappingEnd => match stack.pop() {
                Some(Frame::List(items)) => Some(Value::List(items)),
                Some(Frame::Map { entries, .. }) => Some(Value::Map(entries)),
                None => return Err("unbalanced YAML collection".to_string()),
            },
            YamlEvent::Alias(_) => return Err(format!("YAML aliases are not supported (line {})", mark.line())),
            YamlEvent::Nothing | YamlEvent::StreamStart | YamlEvent::StreamEnd | YamlEvent::DocumentEnd => None,
        };
        let Some(value) = finished else { continue };
        match stack.last_mut() {
            None => root = Some(value),
            Some(Frame::List(items)) => items.push(value),
            Some(Frame::Map { entries, key }) => match key.take() {
                None => {
                    let Value::Str(k) = value else {
                        return Err("a mapping key must be a plain scalar".to_string());
                    };
                    if entries.iter().any(|(existing, _)| *existing == k) {
                        return Err(format!("duplicate key `{k}` at line {}", mark.line()));
                    }
                    *key = Some(k);
                }
                Some(k) => entries.push((k, value)),
            },
        }
    }
    match root {
        None => Ok(Value::Map(Vec::new())),
        Some(v @ Value::Map(_)) => Ok(v),
        Some(_) => Err("frontmatter must be a mapping of keys to values".to_string()),
    }
}

/// An open collection while [`parse_frontmatter`] assembles the event stream into a [`Value`].
enum Frame {
    List(Vec<Value>),
    /// `key` holds a parsed key whose value has not arrived yet.
    Map { entries: Vec<(String, Value)>, key: Option<String> },
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

/// `YYYY-MM-DD` naming a day that exists in the Gregorian calendar.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !digits(&b[..4]) || !digits(&b[5..7]) || !digits(&b[8..10]) {
        return false;
    }
    let year: u32 = s[..4].parse().unwrap_or(0);
    let month: u32 = s[5..7].parse().unwrap_or(0);
    let day: u32 = s[8..10].parse().unwrap_or(0);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days_in_month).contains(&day)
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

/// What the checks read from a markdown body, gathered in one pass of a CommonMark parse.
#[derive(Default)]
pub struct MarkdownFacts {
    /// Relative link and image targets without `#fragment`, in document order. External URLs, pure
    /// fragments and `mailto:` links are skipped. Inline, reference-style and angle-bracket
    /// destinations all resolve to their target; links inside code never appear.
    pub links: Vec<String>,
    /// Text of every inline code span.
    pub code_spans: Vec<String>,
    /// Whitespace-separated words outside code blocks (diagrams and examples do not count).
    pub words: usize,
}

impl MarkdownFacts {
    /// `docs/...md` paths cited inside inline code spans. A span holding a placeholder (`*`, `<`)
    /// or a space is a pattern, not a path.
    pub fn cited_doc_paths(&self) -> impl Iterator<Item = &str> {
        self.code_spans
            .iter()
            .map(String::as_str)
            .filter(|s| s.starts_with("docs/") && s.ends_with(".md") && !s.contains([' ', '*', '<']))
    }
}

/// Reads `body` as CommonMark with tables, as GitHub renders it.
pub fn markdown_facts(body: &str) -> MarkdownFacts {
    let mut facts = MarkdownFacts::default();
    // Everything before this offset is already counted or belongs to a code block.
    let mut counted_to = 0;
    for (event, range) in Parser::new_ext(body, Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            // An email autolink's destination has no `mailto:` prefix, so it is skipped by type.
            Event::Start(Tag::Link { link_type: LinkType::Email, .. }) => {}
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                if let Some(target) = relative_target(&dest_url) {
                    facts.links.push(target);
                }
            }
            Event::Code(text) => facts.code_spans.push(text.into_string()),
            // The range of a block's start event spans the whole block, fences included.
            Event::Start(Tag::CodeBlock(_)) => {
                facts.words += body[counted_to.min(range.start)..range.start].split_whitespace().count();
                counted_to = range.end;
            }
            _ => {}
        }
    }
    facts.words += body[counted_to..].split_whitespace().count();
    facts
}

/// The file part of a link destination, or `None` for a pure fragment or an external link.
fn relative_target(dest: &str) -> Option<String> {
    let target = dest.split('#').next().unwrap_or("");
    let external = target.contains("://") || target.starts_with("mailto:");
    (!target.is_empty() && !external).then(|| target.to_string())
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
    /// What the checks read from `body`, parsed once.
    facts: MarkdownFacts,
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
        let facts = markdown_facts(&body);
        docs.push(Doc { rel, abs, body, has_block, fm, facts });
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
/// - Every generated `index.md` matches what `cargo xtask docs --fix` would write.
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
        for target in &doc.facts.links {
            let resolved = resolve(dir, target);
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
        for cited in doc.facts.cited_doc_paths() {
            if seen.insert(cited) && !root.join(cited).exists() {
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
        let words = doc.facts.words;
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
            Some(_) => report.error(&rel, "out of date with the documents it lists; run `cargo xtask docs --fix`".to_string()),
            None => report.error(&rel, "missing; run `cargo xtask docs --fix`".to_string()),
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
        if CITATION_EXTENSIONS.contains(&ext) && !CITATION_SKIP_FILES.contains(&file_name(path)) {
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
