use crate::search::Query;
use base64::Engine;
use serde_json::Value;
use std::{borrow::Cow, cmp::Ordering, collections::BTreeSet};

#[derive(Clone, Debug)]
pub struct Capture {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
    original: Value,
    wrapped: bool,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: usize,
    pub method: String,
    pub url: String,
    pub host: String,
    pub path: String,
    pub status: u16,
    pub kind: String,
    pub size: Option<u64>,
    pub duration_ms: f64,
    pub start_ms: Option<f64>,
    pub raw: Value,
}

#[derive(Default, Clone, Debug)]
pub struct Filter {
    /// ANDed literal terms; text:/request:/response: expand the default URL/method/status scope.
    /// Double quotes group phrases; a leading minus excludes a term. Matching ignores case.
    pub query: String,
    /// HTTP status classes, 0 (failed) through 5.
    pub statuses: BTreeSet<u16>,
    pub methods: BTreeSet<String>,
    pub kinds: BTreeSet<String>,
    pub hosts: BTreeSet<String>,
    pub slow_only: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SortColumn {
    Start,
    Method,
    Url,
    Status,
    Kind,
    Size,
    Duration,
}

#[derive(Default, Clone, Debug)]
pub struct Summary {
    pub requests: usize,
    pub errors: usize,
    pub bytes: u64,
    pub duration_ms: f64,
    pub average_ms: f64,
}

#[derive(Clone, Debug)]
pub struct Phase {
    pub name: &'static str,
    pub duration_ms: f64,
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn positive(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite() && *n >= 0.0)
}
fn span<'a>(entries: impl Iterator<Item = &'a Entry>) -> (f64, f64) {
    let mut start = f64::INFINITY;
    let mut end = f64::NEG_INFINITY;
    for e in entries {
        if let Some(t) = e.start_ms {
            start = start.min(t);
            end = end.max(t + e.duration_ms);
        }
    }
    if start.is_finite() {
        (start, end)
    } else {
        (0.0, 0.0)
    }
}

fn unicode_escape(bytes: &[u8]) -> Option<u16> {
    bytes
        .strip_prefix(b"\\u")?
        .get(..4)?
        .iter()
        .try_fold(0, |value, &digit| {
            Some(value * 16 + (digit as char).to_digit(16)? as u16)
        })
}

/// JavaScript can emit unpaired UTF-16 surrogates, but Rust strings cannot hold
/// them. Retry with replacements in values only; serde_json still validates all
/// syntax. Never replace keys, since distinct keys could collapse to the same one.
fn parse_json(input: &str) -> Result<(Value, usize), String> {
    let error = match serde_json::from_str(input) {
        Ok(value) => return Ok((value, 0)),
        Err(error) => error,
    };
    let bytes = input.as_bytes();
    let mut normalized = String::new();
    let mut replacements = 0;
    let mut copied = 0;
    let mut in_string = false;
    let mut changed_string = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' if !in_string => {
                in_string = true;
                changed_string = false;
                i += 1;
            }
            b'"' => {
                if changed_string
                    && bytes[i + 1..].iter().find(|b| !b.is_ascii_whitespace()) == Some(&b':')
                {
                    return Err("Invalid HAR JSON: unpaired UTF-16 surrogate in an object key; refusing replacement because distinct keys could merge".into());
                }
                in_string = false;
                i += 1;
            }
            b'\\' if in_string => {
                if let Some(code) = unicode_escape(&bytes[i..]) {
                    if (0xd800..=0xdbff).contains(&code)
                        && unicode_escape(&bytes[i + 6..])
                            .is_some_and(|next| (0xdc00..=0xdfff).contains(&next))
                    {
                        i += 12;
                        continue;
                    }
                    if (0xd800..=0xdfff).contains(&code) {
                        normalized.push_str(&input[copied..i]);
                        // Keep the byte width unchanged so JSON error locations
                        // still refer to the original input.
                        normalized.push_str("\\ufffd");
                        copied = i + 6;
                        replacements += 1;
                        changed_string = true;
                    }
                    i += 6;
                } else {
                    // Consume escaped quotes/backslashes together. In particular,
                    // a literal backslash followed by uD800 is not a surrogate.
                    i += 2;
                }
            }
            _ => i += 1,
        }
    }
    if replacements == 0 {
        return Err(format!("Invalid HAR JSON: {error}"));
    }
    normalized.push_str(&input[copied..]);
    serde_json::from_str(&normalized)
        .map(|value| (value, replacements))
        .map_err(|e| format!("Invalid HAR JSON: {e}"))
}

impl Capture {
    pub fn parse(input: &str) -> Result<Self, String> {
        let (mut original, replacements) = parse_json(input)?;
        if !original.is_object() {
            return Err("HAR root must be an object".into());
        }
        let wrapped = original.get("log").is_some();
        let log = if wrapped { &original["log"] } else { &original };
        let raws = log
            .get("entries")
            .and_then(Value::as_array)
            .ok_or("HAR log must contain an entries array")?;
        let mut entries = Vec::with_capacity(raws.len());
        let mut warnings = Vec::new();
        if replacements > 0 {
            warnings.push(format!(
                "Replaced {replacements} unpaired UTF-16 surrogate escapes with �. Displayed values, clipboard copies and exports contain replacements, not the original code units. The source file is unchanged."
            ));
        }
        for (id, raw) in raws.iter().enumerate() {
            if !raw.is_object() {
                warnings.push(format!("Entry {id}: expected an object; skipped"));
                continue;
            }
            let url = text(&raw["request"]["url"]).to_owned();
            let method = raw["request"]["method"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("?")
                .to_owned();
            let parsed_url = url::Url::parse(&url).ok();
            let host = parsed_url
                .as_ref()
                .map(|u| {
                    let mut h = u.host_str().unwrap_or("").to_owned();
                    if let Some(p) = u.port() {
                        h.push_str(&format!(":{p}"));
                    }
                    h
                })
                .unwrap_or_default();
            let path = parsed_url
                .as_ref()
                .map(|u| {
                    let mut p = u.path().to_owned();
                    if let Some(q) = u.query() {
                        p.push('?');
                        p.push_str(q);
                    }
                    p
                })
                .unwrap_or_else(|| url.clone());
            let status = raw["response"]["status"]
                .as_u64()
                .filter(|s| *s <= 599)
                .unwrap_or(0) as u16;
            let start_ms = raw["startedDateTime"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.timestamp_micros() as f64 / 1000.0);
            let mut e = Entry {
                id,
                method,
                url,
                host,
                path,
                status,
                kind: String::new(),
                size: raw["response"]["content"]["size"]
                    .as_u64()
                    .or_else(|| raw["response"]["bodySize"].as_u64()),
                duration_ms: 0.0,
                start_ms,
                raw: raw.clone(),
            };
            e.duration_ms = positive(&raw["time"])
                .unwrap_or_else(|| e.phases().iter().map(|p| p.duration_ms).sum());
            let headers = e.headers(true);
            let mime = headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| v.as_str())
                .unwrap_or_else(|| text(&raw["response"]["content"]["mimeType"]))
                .to_lowercase();
            e.kind = [
                ("json", "json"),
                ("javascript", "js"),
                ("ecmascript", "js"),
                ("css", "css"),
                ("html", "html"),
                ("image", "img"),
                ("xml", "xml"),
                ("font", "font"),
                ("video", "video"),
                ("audio", "audio"),
                ("text", "text"),
                ("form", "form"),
                ("octet", "bin"),
                ("wasm", "bin"),
            ]
            .iter()
            .find(|(pattern, _)| mime.contains(pattern))
            .map(|(_, kind)| *kind)
            .unwrap_or("other")
            .into();
            let mut defects = Vec::new();
            if !raw["request"].is_object() || e.method == "?" {
                defects.push("missing or invalid request/method");
            }
            if parsed_url.is_none() || e.url.is_empty() {
                defects.push("missing or invalid URL");
            }
            if !raw["response"].is_object()
                || raw["response"]["status"].as_u64().is_none_or(|s| s > 599)
            {
                defects.push("missing or invalid response/status");
            }
            if start_ms.is_none() {
                defects.push("missing or invalid timestamp");
            }
            if positive(&raw["time"]).is_none() {
                defects.push("missing or invalid time; using timing phases");
            }
            if let Some(timings) = raw.get("timings")
                && timings.as_object().is_none_or(|t| {
                    [
                        "blocked", "dns", "connect", "ssl", "send", "wait", "receive",
                    ]
                    .iter()
                    .any(|key| {
                        t.get(*key)
                            .is_some_and(|v| v.as_f64().is_none_or(|n| !n.is_finite() || n < -1.0))
                    })
                })
            {
                defects.push("malformed timing details");
            }
            for value in [
                raw["response"].get("bodySize"),
                raw["response"]["content"].get("size"),
            ]
            .into_iter()
            .flatten()
            {
                if value.as_u64().is_none() && value.as_i64() != Some(-1) {
                    defects.push("malformed response size");
                }
            }
            for side in ["request", "response"] {
                if let Some(h) = raw[side].get("headers")
                    && h.as_array().is_none_or(|a| {
                        a.iter()
                            .any(|h| !h["name"].is_string() || !h["value"].is_string())
                    })
                {
                    defects.push("malformed headers");
                }
            }
            if !defects.is_empty() {
                warnings.push(format!("Entry {id}: {}", defects.join("; ")));
            }
            entries.push(e);
        }
        // Entries own the original request/response JSON. Keep only capture metadata
        // here so bodies aren't retained twice in memory or cloned twice on export.
        let metadata = if wrapped {
            &mut original["log"]
        } else {
            &mut original
        };
        metadata["entries"] = Value::Array(Vec::new());
        Ok(Self {
            entries,
            warnings,
            original,
            wrapped,
        })
    }

    pub fn sample() -> Self {
        Self::parse(include_str!("../fixtures/sample.har")).expect("bundled sample is valid HAR")
    }
    pub fn duration_ms(&self) -> f64 {
        let (s, e) = span(self.entries.iter());
        e - s
    }
    /// Missing timestamps have no known offset and are displayed at zero.
    pub fn offset_ms(&self, entry: &Entry) -> f64 {
        entry
            .start_ms
            .map(|t| (t - span(self.entries.iter()).0).max(0.0))
            .unwrap_or(0.0)
    }

    /// Returns vector indices, not original HAR ids. Unknown sizes/starts sort last in either direction.
    pub fn filtered(&self, filter: &Filter, column: SortColumn, descending: bool) -> Vec<usize> {
        let query = Query::parse(&filter.query);
        let mut indices: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                (filter.statuses.is_empty() || filter.statuses.contains(&(e.status / 100)))
                    && (filter.methods.is_empty() || filter.methods.contains(&e.method))
                    && (filter.kinds.is_empty() || filter.kinds.contains(&e.kind))
                    && (filter.hosts.is_empty() || filter.hosts.contains(&e.host))
                    && (!filter.slow_only || e.duration_ms >= 1000.0)
                    && query.matches(e)
            })
            .map(|(i, _)| i)
            .collect();
        fn optional<T: PartialOrd>(a: Option<T>, b: Option<T>, desc: bool) -> Ordering {
            match (a, b) {
                (Some(a), Some(b)) => {
                    let c = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
                    if desc { c.reverse() } else { c }
                }
                (None, None) => Ordering::Equal,
                (None, _) => Ordering::Greater,
                (_, None) => Ordering::Less,
            }
        }
        indices.sort_by(|&a, &b| {
            let (a, b) = (&self.entries[a], &self.entries[b]);
            match column {
                SortColumn::Start => optional(a.start_ms, b.start_ms, descending),
                SortColumn::Size => optional(a.size, b.size, descending),
                _ => {
                    let c = match column {
                        SortColumn::Method => a.method.cmp(&b.method),
                        SortColumn::Url => a.url.cmp(&b.url),
                        SortColumn::Status => a.status.cmp(&b.status),
                        SortColumn::Kind => a.kind.cmp(&b.kind),
                        _ => a.duration_ms.total_cmp(&b.duration_ms),
                    };
                    if descending { c.reverse() } else { c }
                }
            }
        });
        indices
    }

    /// Invalid selection indices are ignored; ordering and duplicates are retained.
    pub fn summary(&self, indices: &[usize]) -> Summary {
        let selected: Vec<_> = indices
            .iter()
            .filter_map(|&i| self.entries.get(i))
            .collect();
        let (s, e) = span(selected.iter().copied());
        Summary {
            requests: selected.len(),
            errors: selected
                .iter()
                .filter(|e| e.status == 0 || e.status >= 400)
                .count(),
            bytes: selected
                .iter()
                .fold(0u64, |sum, e| sum.saturating_add(e.size.unwrap_or(0))),
            duration_ms: e - s,
            average_ms: if selected.is_empty() {
                0.0
            } else {
                selected.iter().map(|e| e.duration_ms).sum::<f64>() / selected.len() as f64
            },
        }
    }

    pub fn export(&self, indices: &[usize]) -> String {
        let mut root = self.original.clone();
        let log = if self.wrapped {
            &mut root["log"]
        } else {
            &mut root
        };
        log["entries"] = Value::Array(
            indices
                .iter()
                .filter_map(|&i| self.entries.get(i).map(|e| e.raw.clone()))
                .collect(),
        );
        serde_json::to_string_pretty(&root).expect("JSON values serialize")
    }
}

impl Entry {
    pub fn headers(&self, response: bool) -> Vec<(String, String)> {
        self.raw[if response { "response" } else { "request" }]["headers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|h| Some((h["name"].as_str()?.into(), h["value"].as_str()?.into())))
            .collect()
    }

    pub fn body(&self, response: bool) -> Result<String, String> {
        let s = self.body_text(response)?;
        Ok(serde_json::from_str::<Value>(&s)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| s.into_owned()))
    }

    /// Decode once without formatting, so literal search keeps the captured whitespace.
    pub(crate) fn body_text(&self, response: bool) -> Result<Cow<'_, str>, String> {
        let body = if response {
            &self.raw["response"]["content"]
        } else {
            &self.raw["request"]["postData"]
        };
        let Some(value) = body.get("text") else {
            return Ok(Cow::Borrowed(""));
        };
        let s = value.as_str().ok_or("Body text is not a string")?;
        let s = match body.get("encoding").and_then(Value::as_str) {
            Some("base64") => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(s)
                    .map_err(|_| "Invalid base64 body")?;
                Cow::Owned(String::from_utf8(bytes).map_err(|_| "Binary body (not UTF-8 text)")?)
            }
            Some(encoding) if !encoding.is_empty() => {
                return Err(format!("Unsupported body encoding: {encoding}"));
            }
            _ => Cow::Borrowed(s),
        };
        if s.chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err("Binary body (contains control bytes)".into());
        }
        Ok(s)
    }

    pub fn phases(&self) -> Vec<Phase> {
        // HAR connect already includes TLS. Split it into additive segments.
        let t = &self.raw["timings"];
        let connect = positive(&t["connect"]).unwrap_or(0.0);
        let ssl = positive(&t["ssl"]).unwrap_or(0.0).min(connect);
        [
            "blocked", "dns", "connect", "ssl", "send", "wait", "receive",
        ]
        .into_iter()
        .filter_map(|name| {
            let duration_ms = match name {
                "connect" => connect - ssl,
                "ssl" => ssl,
                _ => positive(&t[name]).unwrap_or(0.0),
            };
            (duration_ms > 0.0).then_some(Phase { name, duration_ms })
        })
        .collect()
    }

    /// Produces text only; never executes a command. NUL cannot be represented in POSIX argv.
    pub fn curl(&self) -> String {
        fn quote(s: &str) -> String {
            format!("'{}'", s.replace('\0', "\\0").replace('\'', "'\\''"))
        }
        let mut args = vec!["curl".into(), "--request".into(), quote(&self.method)];
        for (name, value) in self.headers(false) {
            args.extend(["--header".into(), quote(&format!("{name}: {value}"))]);
        }
        if let Some(body) = self.raw["request"]["postData"]["text"].as_str() {
            args.extend(["--data-raw".into(), quote(body)]);
        }
        args.extend(["--url".into(), quote(&self.url)]);
        args.join(" ")
    }

    pub fn issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.status == 0 {
            issues.push("Request failed or did not complete".into());
        } else if self.status >= 400 {
            issues.push(format!("HTTP {} error", self.status));
        }
        if self.duration_ms >= 1000.0 {
            issues.push("Slow request (>= 1000 ms)".into());
        }
        if positive(&self.raw["timings"]["wait"]).unwrap_or(0.0) > 800.0 {
            issues.push("Slow server response (TTFB > 800 ms)".into());
        }
        if self.size.unwrap_or(0) > 1024 * 1024 {
            issues.push("Large response (> 1 MiB)".into());
        }
        if self.kind == "img" && self.size.unwrap_or(0) > 200 * 1024 {
            issues.push("Oversized image (> 200 KiB)".into());
        }
        let h = self.headers(true);
        if (200..400).contains(&self.status)
            && self.status != 304
            && ["js", "css", "img", "font"].contains(&self.kind.as_str())
            && !h.iter().any(|(n, _)| {
                ["cache-control", "expires", "etag"]
                    .iter()
                    .any(|key| n.eq_ignore_ascii_case(key))
            })
        {
            issues.push("Static asset without cache headers".into());
        }
        issues
    }
}
