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
fn fulltext_scopes_cover_request_and_response_fields_and_combine_with_facets() {
    let mut a = entry("https://a.test/checkout", 422, 12, 1500.0);
    a["request"]["method"] = json!("POST");
    a["request"]["headers"] = json!([{"name":"X-Trace","value":"request-marker"}]);
    a["request"]["cookies"] = json!([{"name":"session","value":"cookie-marker"}]);
    a["request"]["queryString"] = json!([{"name":"q","value":"query-marker"}]);
    a["request"]["postData"] = json!({"text":"{\"code\":\"ÄPFEL\"}",
        "params":[{"name":"upload","value":"form-marker","fileName":"receipt.csv"}]});
    a["response"]["headers"] = json!([{"name":"X-Result","value":"response-marker"}]);
    a["response"]["cookies"] = json!([{"name":"result","value":"response-cookie"}]);
    a["response"]["statusText"] = json!("Unprocessable Content");
    a["response"]["redirectURL"] = json!("https://redirect.test/destination");
    a["response"]["content"]["text"] = json!("coupon expired");
    a["response"]["_custom"] = json!({"nested":["extension-marker",true,9876]});
    a["comment"] = json!("outside-marker");
    let mut b = entry("https://b.test/checkout", 200, 5, 999.0);
    b["request"]["postData"] = json!({"text":"coupon expired"});
    b["response"]["content"]["text"] = json!("request-marker");
    let c = capture(vec![a, b]);
    for (query, expected) in [
        ("coupon", vec![]), // Bare text keeps the quick metadata search.
        ("text:coupon", vec![0, 1]),
        ("request:coupon", vec![1]),
        ("response:coupon", vec![0]),
        ("REQUEST:äpfel", vec![0]),
        ("request:request-marker", vec![0]),
        ("response:request-marker", vec![1]),
        ("request:x-trace", vec![0]),
        ("request:cookie-marker", vec![0]),
        ("request:query-marker", vec![0]),
        ("request:form-marker", vec![0]),
        ("request:receipt.csv", vec![0]),
        ("response:response-cookie", vec![0]),
        ("response:response-marker", vec![0]),
        ("response:\"unprocessable content\"", vec![0]),
        ("response:redirect.test", vec![0]),
        ("response:application/json", vec![0, 1]),
        ("text:extension-marker", vec![0]),
        ("text:_custom", vec![0]),
        ("text:true", vec![0]),
        ("text:9876", vec![0]),
        ("text:outside-marker", vec![]),
        ("POST text:coupon -response:request-marker", vec![0]),
        ("request:request-marker response:request-marker", vec![]),
        ("-text:coupon", vec![]),
    ] {
        let f = Filter {
            query: query.into(),
            ..Default::default()
        };
        assert_eq!(
            c.filtered(&f, SortColumn::Start, false),
            expected,
            "{query}"
        );
    }
    let mut f = Filter {
        query: "text:coupon".into(),
        statuses: [2, 4].into(),
        methods: ["POST".into()].into(),
        kinds: ["json".into()].into(),
        hosts: ["a.test".into()].into(),
        slow_only: true,
    };
    assert_eq!(c.filtered(&f, SortColumn::Duration, true), [0]);
    f.statuses = [2].into();
    assert!(c.filtered(&f, SortColumn::Start, false).is_empty());
}

#[test]
fn search_phrases_escaping_literals_and_incomplete_input() {
    let mut a = entry("https://a.test:8443/error:code", 200, 0, 0.0);
    a["response"]["content"]["text"] =
        json!(r#"warm light; say "hello"; C:\temp; text:literal; -draft"#);
    let mut b = entry("https://b.test/path", 503, 0, 0.0);
    b["response"]["content"]["text"] = json!("light is warm");
    let c = capture(vec![a, b]);
    for (query, expected) in [
        ("text:warm text:light", vec![0, 1]),
        (r#"text:"warm light""#, vec![0]),
        (r#"text:"warm light" -503"#, vec![0]),
        (r#"-text:"warm light""#, vec![1]),
        (r#"text:"say \"hello\"""#, vec![0]),
        (r#"text:"C:\\temp""#, vec![0]),
        (r"text:C:\temp", vec![0]),
        ("text:text:literal", vec![0]),
        ("text:-draft", vec![0]),
        ("error:code", vec![0]), // Unknown prefixes and URL colons are literal.
        ("https://a.test:8443", vec![0]),
        ("GET 503", vec![1]),
        ("\t text:warm   -503 \n", vec![0]),
        (r#"text:"warm light"#, vec![0]), // Open quotes consume the remaining input.
        ("text:", vec![0, 1]),            // Empty terms are ignored while typing.
        ("-response:", vec![0, 1]),
        (r#"text:"" """#, vec![0, 1]),
        ("  \t\n", vec![0, 1]),
        ("-", vec![]),              // A lone minus remains literal.
        (r#""text:warm""#, vec![]), // Quoting a whole token disables its prefix.
    ] {
        let f = Filter {
            query: query.into(),
            ..Default::default()
        };
        assert_eq!(
            c.filtered(&f, SortColumn::Start, false),
            expected,
            "{query}"
        );
    }
}

#[test]
fn fulltext_search_uses_complete_decoded_bodies_not_preview_or_base64_data() {
    let bodies = [
        json!({"text":"Äpfel marker"}),
        json!({"encoding":"base64","text":"w4RwZmVsIG1hcmtlcg=="}),
        json!({"encoding":"base64","text":"%%% marker"}),
        json!({"encoding":"base64","text":"/w=="}),
        json!({"encoding":"base64","text":"bWFya2VyAA=="}),
        json!({"encoding":"gzip","text":"marker"}),
        json!({"text":42}),
        json!({}),
        json!({"text":format!("{}{}tail-marker", "line\n".repeat(5100), "x".repeat(2100))}),
        json!({"text":"{\"compact\":true}"}),
    ];
    for (side, field) in [("request", "postData"), ("response", "content")] {
        let c = capture(
            bodies
                .iter()
                .map(|body| {
                    let mut e = entry("https://a.test/", 200, 0, 0.0);
                    e[side][field] = body.clone();
                    e
                })
                .collect(),
        );
        for (query, expected) in [
            ("text:äpfel", vec![0, 1]),
            ("text:marker", vec![0, 1, 8]),
            ("text:base64", vec![1, 2, 3, 4]), // Encoding metadata stays searchable.
            ("text:gzip", vec![5]),
            ("text:w4rwzmvs", vec![]),
            ("text:bwfya2vy", vec![]),
            ("text:binary", vec![]), // Decoder errors are not captured content.
            ("text:tail-marker", vec![8]),
            (r#"text:"{\"compact\":true}""#, vec![9]),
        ] {
            let f = Filter {
                query: query.into(),
                ..Default::default()
            };
            assert_eq!(
                c.filtered(&f, SortColumn::Start, false),
                expected,
                "{side} {query}"
            );
        }
    }
    let c = capture(vec![json!({}), json!({"request":42,"response":null})]);
    let f = Filter {
        query: "text:marker".into(),
        ..Default::default()
    };
    assert!(c.filtered(&f, SortColumn::Start, false).is_empty());
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
fn unpaired_surrogate_values_load_with_an_export_warning() {
    let mut raw = entry("https://a.test", 200, 0, 1.0);
    raw["response"]["content"]["text"] = json!("BODY");
    for (encoded, expected, replacements) in [
        (r"\udce9", "�", 1),
        (r"\uD800x\uDC00", "�x�", 2),
        (r"\uD800\u0041", "�A", 1),
        (r"\uD800\uD83D\uDE80", "�🚀", 1),
        (r"\uD83D\uDE80\udfff", "🚀�", 1),
        (r"\uDBFF\uDFFF", "\u{10ffff}", 0),
        (r"\uD7FF\uE000", "\u{d7ff}\u{e000}", 0),
        (r"\\uD800", r"\uD800", 0),
        (r"\\\uD800", "\\�", 1),
        (r#"before\"\ud800:after"#, "before\"�:after", 1),
        (r"é\udc00\t尾", "é�\t尾", 1),
    ] {
        let input = json!({"entries":[raw]})
            .to_string()
            .replace("BODY", encoded);
        let c = Capture::parse(&input).unwrap();
        assert_eq!(c.entries[0].body(true).unwrap(), expected, "{encoded}");
        let output: Value = serde_json::from_str(&c.export(&[0])).unwrap();
        assert_eq!(
            output["entries"][0]["response"]["content"]["text"],
            expected
        );
        if replacements == 0 {
            assert!(c.warnings.is_empty(), "{encoded}");
        } else {
            assert_eq!(c.warnings.len(), 1, "{encoded}");
            assert!(c.warnings[0].contains(&format!("{replacements} unpaired")));
            assert!(c.warnings[0].contains("exports"));
            assert!(c.warnings[0].contains("source file"));
        }
    }
    let input = r#"{"_note":"\ud800","log":{"entries":[],"comment":"\udfff"}}"#;
    let c = Capture::parse(input).unwrap();
    assert!(c.warnings[0].contains("2 unpaired"));
    let output: Value = serde_json::from_str(&c.export(&[])).unwrap();
    assert_eq!(output["_note"], "�");
    assert_eq!(output["log"]["comment"], "�");
}

#[test]
fn surrogate_recovery_preserves_keys_and_rejects_other_invalid_json() {
    for input in [
        r#"{"entries":[],"\ud800":1,"\ud801":2,"\ufffd":3}"#,
        r#"{"entries":[],"_extension":{"\udc00" :1}}"#,
    ] {
        let error = Capture::parse(input).unwrap_err();
        assert!(error.contains("object key"), "{error}");
    }
    let c =
        Capture::parse(r#"{"entries":[],"\ud83d\ude80":1,"\\ud800":2,"_note":"\udfff"}"#).unwrap();
    let output: Value = serde_json::from_str(&c.export(&[])).unwrap();
    assert_eq!(output["🚀"], 1);
    assert_eq!(output[r"\ud800"], 2);
    for input in [
        r#"{"entries":[],"text":"\ud800",}"#,
        r#"{"entries":[],"text":"\ud800\x"}"#,
        r#"{"entries":[],"text":"\ud800\uQQQQ"}"#,
        r#"{"entries":[],"text":"\ud800\u12"}"#,
        r#"{"entries":[],"text":"\ud800"} trailing"#,
        r#"{"entries":[],"text":"\ud800"#,
        r#"{"entries":[],"text":\ud800}"#,
        "{\"entries\":[],\"text\":\"\\ud800\n\"}",
    ] {
        assert!(Capture::parse(input).is_err(), "{input}");
    }
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
