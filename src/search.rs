//! Literal search terms shared by filtering, independent of the desktop UI.
use crate::har::Entry;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Summary,
    Text,
    Request,
    Response,
}

struct Term {
    scope: Scope,
    needle: String,
    exclude: bool,
}

pub(crate) struct Query(Vec<Term>);

impl Query {
    pub(crate) fn parse(input: &str) -> Self {
        let input = input.to_lowercase();
        let mut rest = input.as_str();
        let mut terms = Vec::new();
        while !rest.trim_start().is_empty() {
            rest = rest.trim_start();
            let exclude = rest
                .strip_prefix('-')
                .is_some_and(|s| s.starts_with(|c: char| !c.is_whitespace()));
            if exclude {
                rest = &rest[1..];
            }
            let scope = [
                ("text:", Scope::Text),
                ("request:", Scope::Request),
                ("response:", Scope::Response),
            ]
            .into_iter()
            .find_map(|(prefix, scope)| {
                rest.strip_prefix(prefix).map(|suffix| {
                    rest = suffix;
                    scope
                })
            })
            .unwrap_or(Scope::Summary);
            let (needle, suffix) = take_term(rest);
            rest = suffix;
            // Empty scopes and open quotes are normal during live input.
            if !needle.is_empty() {
                terms.push(Term {
                    scope,
                    needle,
                    exclude,
                });
            }
        }
        // Reject cheap URL/method/status mismatches before scanning bodies.
        terms.sort_by_key(|term| term.scope != Scope::Summary);
        Self(terms)
    }

    pub(crate) fn matches(&self, entry: &Entry) -> bool {
        self.0.iter().all(|term| {
            let found = match term.scope {
                Scope::Summary => {
                    contains(&entry.url, &term.needle)
                        || contains(&entry.method, &term.needle)
                        || entry.status.to_string().contains(&term.needle)
                }
                Scope::Text => {
                    matches_side(entry, false, &term.needle)
                        || matches_side(entry, true, &term.needle)
                }
                Scope::Request => matches_side(entry, false, &term.needle),
                Scope::Response => matches_side(entry, true, &term.needle),
            };
            found != term.exclude
        })
    }
}

fn take_term(input: &str) -> (String, &str) {
    let mut value = String::new();
    let mut quoted = false;
    let mut chars = input.char_indices().peekable();
    while let Some((offset, c)) = chars.next() {
        match c {
            c if c.is_whitespace() && !quoted => return (value, &input[offset..]),
            '"' => quoted = !quoted,
            '\\' if quoted && matches!(chars.peek(), Some((_, '"' | '\\'))) => {
                value.push(chars.next().unwrap().1);
            }
            _ => value.push(c),
        }
    }
    (value, "")
}

fn contains(value: &str, needle: &str) -> bool {
    value.to_lowercase().contains(needle)
}

fn matches_side(entry: &Entry, response: bool, needle: &str) -> bool {
    let (side, body) = if response {
        ("response", "content")
    } else {
        ("request", "postData")
    };
    entry.raw[side].as_object().is_some_and(|fields| {
        fields.iter().any(|(name, value)| {
            contains(name, needle)
                || if name == body {
                    // Never search encoded bytes or generated decoder errors. Use
                    // the complete decoded text, not the pretty/truncated preview.
                    matches_value(value, needle, Some("text"))
                        || entry
                            .body_text(response)
                            .is_ok_and(|s| contains(&s, needle))
                } else {
                    matches_value(value, needle, None)
                }
        })
    })
}

fn matches_value(value: &Value, needle: &str, skip: Option<&str>) -> bool {
    match value {
        Value::String(s) => contains(s, needle),
        Value::Array(values) => values.iter().any(|v| matches_value(v, needle, None)),
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            contains(key, needle)
                || (Some(key.as_str()) != skip && matches_value(value, needle, None))
        }),
        Value::Number(n) => n.to_string().contains(needle),
        Value::Bool(b) => b.to_string().contains(needle),
        Value::Null => false,
    }
}
