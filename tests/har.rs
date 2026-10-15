use har_viewer::har::{Capture, Filter, SortColumn};
use serde_json::{Value, json};

fn entry(url: &str, status: u16, size: i64, time: f64) -> Value {
    json!({"startedDateTime":"2026-01-01T00:00:00Z","time":time,
        "request":{"method":"GET","url":url,"headers":[]},
        "response":{"status":status,"headers":[],"content":{"size":size,"mimeType":"application/json"}},
        "timings":{"connect":30,"ssl":20,"send":1,"wait":60,"receive":9}})
}
fn capture(entries: Vec<Value>) -> Capture {
    Capture::parse(&json!({"entries":entries}).to_string()).unwrap()
}

#[test]
fn roots_empty_and_partial_entries() {
    for input in [
        "",
        "null",
        "[]",
        "1",
        "{}",
        "{\"log\":null}",
        "{\"entries\":{}}",
        "{\"log\":{},\"entries\":[]}",
    ] {
        assert!(Capture::parse(input).is_err(), "{input}");
    }
    for input in ["{\"entries\":[]}", "{\"log\":{\"entries\":[]}}"] {
        let c = Capture::parse(input).unwrap();
        assert!(c.entries.is_empty());
        assert_eq!(c.duration_ms(), 0.0);
        assert_eq!(c.summary(&[]).average_ms, 0.0);
        assert!(Capture::parse(&c.export(&[])).unwrap().entries.is_empty());
    }
    let c = capture(vec![
        Value::Null,
        json!({}),
        json!({"request":42,"response":{"status":"bad","headers":[null]},"timings":{"ssl":-1}}),
        entry("https://valid.test", 200, 0, 0.0),
    ]);
    assert_eq!(c.entries.len(), 3);
    assert_eq!(c.warnings.len(), 3);
    assert_eq!(c.entries[0].id, 1);
    assert_eq!(c.entries[0].status, 0);
    assert!(c.entries[0].start_ms.is_none());
    assert!(c.entries[1].headers(true).is_empty());
    assert_eq!(
        c.filtered(&Filter::default(), SortColumn::Start, false)[0],
        2
    );
    assert_eq!(c.summary(&[0, 1, 2, 999]).errors, 2);
}

#[test]
fn filters_and_dimensions_or_within_and_unicode_query() {
    let mut a = entry("https://a.test/ÄPFEL", 200, 12, 1000.0);
    a["request"]["method"] = json!("POST");
    let mut b = entry("https://b.test/Äpfel", 503, 12, 2000.0);
    b["request"]["method"] = json!("POST");
    let c = capture(vec![
        a,
        b,
        entry("https://a.test/Äpfel", 200, 5, 999.0),
        entry("https://a.test/no", 404, 7, 1400.0),
    ]);
    let mut f = Filter {
        query: "äpfel".into(),
        statuses: [2, 5].into(),
        methods: ["GET".into(), "POST".into()].into(),
        kinds: ["json".into(), "text".into()].into(),
        hosts: ["a.test".into(), "b.test".into()].into(),
        slow_only: true,
    };
    assert_eq!(c.filtered(&f, SortColumn::Duration, false), [0, 1]);
    f.hosts = ["a.test".into()].into();
    assert_eq!(c.filtered(&f, SortColumn::Duration, true), [0]);
    f.query = "503".into();
    assert!(c.filtered(&f, SortColumn::Start, false).is_empty());
    f.hosts.clear();
    assert_eq!(c.filtered(&f, SortColumn::Start, false), [1]);
    f.query = "post".into();
    assert_eq!(c.filtered(&f, SortColumn::Start, false), [0, 1]);
    f.query = "application/json".into();
    assert!(c.filtered(&f, SortColumn::Start, false).is_empty());
    let failed = capture(vec![entry("https://a.test", 0, 0, 1000.0)]);
    assert_eq!(
        failed.filtered(
            &Filter {
                statuses: [0].into(),
                ..Default::default()
            },
            SortColumn::Status,
            false
        ),
        [0]
    );
}

#[test]
fn size_unknown_zero_fallback_and_stable_sort() {
    let mut fallback = entry("https://a.test", 200, -1, 1.0);
    fallback["response"]["bodySize"] = json!(7);
    let c = capture(vec![
        entry("https://a.test", 200, -1, 1.0),
        entry("https://a.test", 200, 0, 1.0),
        fallback,
        entry("https://a.test", 200, 7, 1.0),
        entry("https://a.test", 200, -1, 1.0),
    ]);
    assert_eq!(c.entries[0].size, None);
    assert_eq!(c.entries[1].size, Some(0));
    assert_eq!(
        c.filtered(&Filter::default(), SortColumn::Size, false),
        [1, 2, 3, 0, 4]
    );
    assert_eq!(
        c.filtered(&Filter::default(), SortColumn::Size, true),
        [2, 3, 1, 0, 4]
    );
    for sort in [
        SortColumn::Start,
        SortColumn::Method,
        SortColumn::Url,
        SortColumn::Status,
        SortColumn::Kind,
        SortColumn::Duration,
    ] {
        assert_eq!(c.filtered(&Filter::default(), sort, true), [0, 1, 2, 3, 4]);
    }
    assert_eq!(c.summary(&[0, 1, 2, 3, 4]).bytes, 14);
}

#[test]
fn timezone_wall_span_and_additive_timings() {
    let mut a = entry("https://a.test:8443/a?q=1#fragment", 200, 1, 100.0);
    a["startedDateTime"] = json!("1970-01-01T01:00:00.000+01:00");
    let mut b = entry("https://b.test", 404, 2, 300.0);
    b["startedDateTime"] = json!("1969-12-31T19:00:00.050-05:00");
    let mut d = entry("broken", 0, -1, -1.0);
    d["startedDateTime"] = json!("not a date");
    d["timings"] =
        json!({"blocked":-1,"dns":-1,"connect":30,"ssl":20,"send":1,"wait":60,"receive":9});
    let c = capture(vec![a, b, d]);
    assert_eq!(c.entries[0].start_ms, Some(0.0));
    assert_eq!(c.offset_ms(&c.entries[1]), 50.0);
    assert_eq!(c.offset_ms(&c.entries[2]), 0.0);
    assert_eq!(c.duration_ms(), 350.0);
    assert_eq!(c.summary(&[0, 1, 2]).duration_ms, 350.0);
    assert_eq!(c.summary(&[0, 1, 2]).average_ms, 500.0 / 3.0);
    assert_eq!(c.entries[0].host, "a.test:8443");
    assert_eq!(c.entries[0].path, "/a?q=1");
    assert_eq!(c.entries[2].duration_ms, 100.0);
    let phases = c.entries[2].phases();
    assert_eq!(phases.iter().map(|p| p.duration_ms).sum::<f64>(), 100.0);
    assert_eq!(
        phases
            .iter()
            .find(|p| p.name == "connect")
            .unwrap()
            .duration_ms,
        10.0
    );
    let mut malformed = entry("https://a.test", 200, 0, -1.0);
    malformed["timings"] = json!({"connect":4,"ssl":80,"wait":"oops"});
    let c = capture(vec![malformed]);
    assert_eq!(c.entries[0].duration_ms, 4.0);
}

#[test]
fn bodies_headers_base64_and_binary() {
    let mut raw = entry("https://a.test", 200, 0, 1.0);
    raw["request"]["headers"] =
        json!([{"name":"X-Duplicate","value":"one"},{"name":"X-Duplicate","value":"two"},null]);
    raw["request"]["postData"] = json!({"text":"{\"a\":1}"});
    raw["response"]["content"] = json!({"encoding":"base64","text":"eyJiIjoyfQ=="});
    let c = capture(vec![raw.clone()]);
    assert_eq!(c.entries[0].headers(false).len(), 2);
    assert!(c.entries[0].body(false).unwrap().contains("\n  \"a\": 1"));
    assert!(c.entries[0].body(true).unwrap().contains("\n  \"b\": 2"));
    for (body, expected) in [
        (json!({"encoding":"base64","text":"%%%"}), "Invalid base64"),
        (json!({"encoding":"base64","text":"/w=="}), "Binary"),
        (json!({"encoding":"base64","text":"AA=="}), "Binary"),
        (json!({"encoding":"gzip","text":"abc"}), "Unsupported"),
        (json!({"text":42}), "not a string"),
    ] {
        raw["response"]["content"] = body;
        assert!(
            capture(vec![raw.clone()]).entries[0]
                .body(true)
                .unwrap_err()
                .contains(expected)
        );
    }
    raw["response"]["content"] = json!({"text":"not JSON"});
    assert_eq!(
        capture(vec![raw.clone()]).entries[0].body(true).unwrap(),
        "not JSON"
    );
    raw["response"]["content"] = json!({});
    assert_eq!(capture(vec![raw]).entries[0].body(true).unwrap(), "");
}

#[test]
fn export_roundtrip_metadata_extensions_and_selected_order() {
    let mut a = entry("https://a.test", 200, 0, 1.0);
    a["_vendor"] = json!({"unknown":[1,null,"x"]});
    let b = entry("https://b.test", 500, 4, 2.0);
    for wrapped in [false, true] {
        let log = json!({"entries":[a,b],"pages":[{"id":"one"}],"_logExtension":{"x":1},"creator":{"name":"test"}});
        let root = if wrapped {
            json!({"log":log,"_root":[42]})
        } else {
            log
        };
        let c = Capture::parse(&root.to_string()).unwrap();
        let output: Value = serde_json::from_str(&c.export(&[1, 0, 999])).unwrap();
        let out_log = if wrapped { &output["log"] } else { &output };
        let in_log = if wrapped { &root["log"] } else { &root };
        assert_eq!(out_log["pages"], in_log["pages"]);
        assert_eq!(out_log["creator"], in_log["creator"]);
        assert_eq!(out_log["_logExtension"], in_log["_logExtension"]);
        assert_eq!(out_log["entries"][1], a);
        assert_eq!(out_log["entries"][0], b);
        assert_eq!(output.get("_root"), root.get("_root"));
        let roundtrip = Capture::parse(&output.to_string()).unwrap();
        assert_eq!(roundtrip.entries[0].status, 500);
        assert_eq!(roundtrip.entries[1].raw["_vendor"], a["_vendor"]);
    }
}

#[test]
fn curl_is_posix_quoted_without_running_network_tools() {
    let mut raw = entry("https://a.test/a'b?q=$(bad)&x=1", 200, 0, 1.0);
    raw["request"]["method"] = json!("POST");
    raw["request"]["headers"] = json!([{"name":"X-Note","value":"it's $HOME; `bad`\nnext"}]);
    raw["request"]["postData"] = json!({"text":"@/etc/passwd ' $(bad)\nline"});
    let c = capture(vec![raw]);
    let e = &c.entries[0];
    let command = e.curl();
    // Replace curl with a shell function that prints arguments. No executable curl or HTTP traffic.
    let script = format!("curl() {{ printf '%s\\000' \"$@\"; }}; {command}");
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .unwrap();
    assert!(output.status.success());
    let args: Vec<_> = output
        .stdout
        .split(|b| *b == 0)
        .filter(|b| !b.is_empty())
        .map(|s| String::from_utf8(s.to_vec()).unwrap())
        .collect();
    assert_eq!(
        args,
        vec![
            "--request",
            "POST",
            "--header",
            "X-Note: it's $HOME; `bad`\nnext",
            "--data-raw",
            "@/etc/passwd ' $(bad)\nline",
            "--url",
            &e.url
        ]
    );
}

#[test]
fn sample_is_realistic_and_issues_are_local() {
    let c = Capture::sample();
    assert_eq!(c.entries.len(), 35);
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(c.duration_ms(), 9300.0);
    assert_eq!(c.summary(&(0..35).collect::<Vec<_>>()).errors, 5);
    assert!(c.entries.last().unwrap().issues().len() >= 2);
    assert!(
        c.entries[4]
            .issues()
            .iter()
            .any(|s| s.contains("Oversized"))
    );
    assert!(c.entries[3].issues().iter().any(|s| s.contains("cache")));
    assert!(c.entries[3].body(true).unwrap_err().contains("Binary"));
    assert!(
        c.entries
            .iter()
            .any(|e| e.body(false).unwrap().contains("quantity"))
    );
}
