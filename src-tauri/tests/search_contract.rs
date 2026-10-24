//! E08.1 search contract: literal full-text substring over text_full via list_recent.
use pinfeather_lib::{
    contracts::*,
    store::{Payload, Snapshot, SourceConfidence, Store},
};
use serde_json::json;

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
}

fn text_clip(id: &str, at: i64, text: &str) -> Snapshot {
    Snapshot {
        capture_id: id.into(),
        captured_at_ms: at,
        source_process: None,
        source_confidence: SourceConfidence::Unknown,
        omitted_formats: vec![],
        payloads: vec![Payload { format_name: "CF_UNICODETEXT".into(), data: utf16(text) }],
    }
}

fn png_clip(id: &str, at: i64) -> Snapshot {
    Snapshot {
        capture_id: id.into(),
        captured_at_ms: at,
        source_process: None,
        source_confidence: SourceConfidence::Unknown,
        omitted_formats: vec![],
        payloads: vec![Payload {
            format_name: "PNG".into(),
            data: b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec(),
        }],
    }
}

fn search_at(store: &Store, query: &str, limit: u32, cursor: Option<ListCursor>) -> ListClipsResult {
    let request =
        parse_list_clips_request(&json!({"request_id": "s", "query": query, "limit": limit, "cursor": cursor}))
            .unwrap();
    store.list_recent(&request).unwrap()
}

fn search(store: &Store, query: &str, limit: u32) -> ListClipsResult {
    search_at(store, query, limit, None)
}

fn previews(result: &ListClipsResult) -> Vec<String> {
    result.summaries.iter().map(|s| s.preview.clone()).collect()
}

#[test]
fn cn01_short_terms_hit_expected_clip() {
    let mut store = Store::open_in_memory().unwrap();
    store.insert(&text_clip("CN01", 1000, "今天讨论剪贴板项目定位")).unwrap();
    for query in ["贴", "定位", "剪贴板", "项目定位"] {
        let page = search(&store, query, 50);
        assert_eq!(previews(&page), vec!["今天讨论剪贴板项目定位"], "{query}");
        assert_eq!(page.request_id, "s");
        assert_eq!(page.normalized_query, query);
        assert!(page.next_cursor.is_none(), "single hit has no follow-up page: {query}");
    }
    let miss = search(&store, "位定", 50);
    assert!(miss.summaries.is_empty() && miss.next_cursor.is_none());
}

#[test]
fn deep_text_beyond_preview_is_searchable() {
    let mut store = Store::open_in_memory().unwrap();
    let text = format!("{}隐藏命中定位", "前".repeat(200));
    let saved = store.insert(&text_clip("DEEP01", 1000, &text)).unwrap();
    let page = search(&store, "隐藏命中定位", 50);
    assert_eq!(page.summaries.len(), 1);
    assert_eq!(page.summaries[0].id, saved.clip_id.to_string());
    assert_eq!(page.summaries[0].preview, "前".repeat(200));
    assert!(!page.summaries[0].preview.contains("隐藏命中定位"), "preview must not contain tail");
    assert_eq!(previews(&search(&store, "命中", 50)), vec!["前".repeat(200)]);
}

#[test]
fn ascii_case_folding_is_ascii_only() {
    let mut store = Store::open_in_memory().unwrap();
    store.insert(&text_clip("ASCII", 1000, "Pinfeather HTTP foo_bar")).unwrap();
    for query in ["pinfeather", "PINFEATHER", "Http", "FOO_BAR", "foo_bar"] {
        assert_eq!(
            previews(&search(&store, query, 50)),
            vec!["Pinfeather HTTP foo_bar"],
            "{query}"
        );
    }
    // Only ASCII case folds; accented pairs stay distinct literals.
    store.insert(&text_clip("LATIN", 900, "École")).unwrap();
    assert!(search(&store, "école", 50).summaries.is_empty());
    assert_eq!(previews(&search(&store, "École", 50)), vec!["École"]);
}

#[test]
fn special_characters_and_url_fragments_are_literal() {
    let mut store = Store::open_in_memory().unwrap();
    store.insert(&text_clip("SPECIAL", 1000, "100% a_b 'quoted' \\path")).unwrap();
    store.insert(&text_clip("URL", 900, "https://example.invalid/a_b?q=100%25")).unwrap();
    store.insert(&text_clip("CONTROL", 800, "axb 100 percent quoted path")).unwrap();
    store.insert(&text_clip("INJECT", 700, "%' OR '1'='1")).unwrap();
    // `_` must not act as a single-char wildcard: axb stays out of a_b.
    assert_eq!(
        previews(&search(&store, "a_b", 50)),
        vec!["100% a_b 'quoted' \\path", "https://example.invalid/a_b?q=100%25"]
    );
    // `%` must not act as a suffix wildcard: 10% is no literal substring of 100%.
    assert!(search(&store, "10%", 50).summaries.is_empty());
    assert_eq!(
        previews(&search(&store, "100%", 50)),
        vec!["100% a_b 'quoted' \\path", "https://example.invalid/a_b?q=100%25"]
    );
    assert_eq!(previews(&search(&store, "100%25", 50)), vec!["https://example.invalid/a_b?q=100%25"]);
    assert_eq!(previews(&search(&store, "'quoted'", 50)), vec!["100% a_b 'quoted' \\path"]);
    assert_eq!(previews(&search(&store, "\\path", 50)), vec!["100% a_b 'quoted' \\path"]);
    assert_eq!(previews(&search(&store, "axb", 50)), vec!["axb 100 percent quoted path"]);
    // Quote/backslash payloads stay literal data, never SQL shape.
    assert_eq!(previews(&search(&store, "%' OR '1'='1", 50)), vec!["%' OR '1'='1"]);
    assert!(search(&store, "OR 1=1 --", 50).summaries.is_empty());
}

#[test]
fn query_whitespace_is_trimmed_at_edges_and_preserved_inside() {
    let mut store = Store::open_in_memory().unwrap();
    store.insert(&text_clip("SP1", 1000, "alpha  beta")).unwrap();
    store.insert(&text_clip("SP2", 900, "alpha beta")).unwrap();
    store.insert(&text_clip("NL1", 800, "alpha\nbeta")).unwrap();
    store.insert(&text_clip("CN2", 700, "剪贴板 定位")).unwrap();
    store.insert(&text_clip("CN3", 600, "剪贴板  定位")).unwrap();
    assert_eq!(previews(&search(&store, "alpha  beta", 50)), vec!["alpha  beta"]);
    assert_eq!(previews(&search(&store, "alpha beta", 50)), vec!["alpha beta"]);
    assert_eq!(previews(&search(&store, "alpha\nbeta", 50)), vec!["alpha\nbeta"]);
    assert_eq!(previews(&search(&store, "剪贴板  定位", 50)), vec!["剪贴板  定位"]);
    assert_eq!(previews(&search(&store, "剪贴板 定位", 50)), vec!["剪贴板 定位"]);
    let trimmed = search(&store, "\u{3000}\u{2003}\t剪贴板 定位\u{a0}\u{85}", 50);
    assert_eq!(trimmed.normalized_query, "剪贴板 定位");
    assert_eq!(previews(&trimmed), vec!["剪贴板 定位"]);
    assert!(trimmed.next_cursor.is_none());
    // U+FEFF is not White_Space and stays part of the query.
    let bom = search(&store, "\u{feff}定位", 50);
    assert_eq!(bom.normalized_query, "\u{feff}定位");
    assert!(bom.summaries.is_empty());
}

#[test]
fn empty_database_no_match_and_png_only_semantics() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(search(&store, "定位", 50).summaries.is_empty());
    assert!(search(&store, "", 50).summaries.is_empty());
    store.insert(&png_clip("PNG1", 1000)).unwrap();
    store.insert(&text_clip("TXT", 900, "有正文定位")).unwrap();
    assert_eq!(previews(&search(&store, "定位", 50)), vec!["有正文定位"]);
    assert!(search(&store, "PNG1", 50).summaries.is_empty());
    assert!(search(&store, "\u{20}", 50).summaries.len() == 2, "whitespace-only query is a recent list");
    let recent = search(&store, "", 50);
    assert_eq!(
        recent
            .summaries
            .iter()
            .map(|s| (s.kind.clone(), s.preview.clone()))
            .collect::<Vec<_>>(),
        vec![("png".to_string(), String::new()), ("text".to_string(), "有正文定位".to_string())]
    );
    assert!(search(&store, "无匹配", 50).summaries.is_empty());
}

#[test]
fn same_ms_results_page_by_id_desc_without_duplicates() {
    let mut store = Store::open_in_memory().unwrap();
    for (id, text) in [("A", "关键A"), ("B", "关键B"), ("C", "关键C")] {
        store.insert(&text_clip(id, 5000, text)).unwrap();
    }
    let mut seen = Vec::new();
    let mut cursor: Option<ListCursor> = None;
    loop {
        let page = search_at(&store, "关键", 1, cursor.clone());
        assert_eq!(page.normalized_query, "关键");
        if page.summaries.is_empty() {
            assert!(page.next_cursor.is_none());
            break;
        }
        assert_eq!(page.summaries.len(), 1);
        seen.push(page.summaries[0].preview.clone());
        cursor = page.next_cursor.clone();
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(seen, vec!["关键C", "关键B", "关键A"]);
    let all = search(&store, "关键", 50);
    assert_eq!(previews(&all), vec!["关键C", "关键B", "关键A"]);
    assert_eq!(
        all.summaries.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["3", "2", "1"],
        "same captured_at_ms orders by id DESC"
    );
    assert!(all.next_cursor.is_none());
}

#[test]
fn insert_after_first_page_with_bigger_id_older_timestamp_stays_out() {
    let mut store = Store::open_in_memory().unwrap();
    for (id, text) in [("A", "关键A"), ("B", "关键B"), ("C", "关键C")] {
        store.insert(&text_clip(id, 5000, text)).unwrap();
    }
    let first = search(&store, "关键", 1);
    assert_eq!(previews(&first), vec!["关键C"]);
    let cursor = first.next_cursor.clone().unwrap();
    assert_eq!(cursor.upper_id, "3");
    assert_eq!(cursor.normalized_query, "关键");
    // Bigger id, older capture time, still a query match.
    store.insert(&text_clip("NEW", 1, "关键NEW")).unwrap();
    let second = search_at(&store, "关键", 1, Some(cursor));
    assert_eq!(previews(&second), vec!["关键B"]);
    let third = search_at(&store, "关键", 1, second.next_cursor.clone());
    assert_eq!(previews(&third), vec!["关键A"]);
    assert!(third.next_cursor.is_none(), "NEW must not leak into later pages");
    let fresh = search(&store, "关键", 50);
    assert_eq!(previews(&fresh), vec!["关键C", "关键B", "关键A", "关键NEW"]);
}

#[test]
fn non_matching_rows_after_last_match_yield_null_cursor() {
    let mut store = Store::open_in_memory().unwrap();
    store.insert(&text_clip("M1", 300, "命中A")).unwrap();
    store.insert(&text_clip("N1", 200, "无关1")).unwrap();
    store.insert(&text_clip("N2", 100, "无关2")).unwrap();
    let page = search(&store, "命中", 1);
    assert_eq!(previews(&page), vec!["命中A"]);
    assert!(page.next_cursor.is_none(), "trailing non-matches must not produce a ghost cursor");
    let all = search(&store, "命中", 50);
    assert_eq!(previews(&all), vec!["命中A"]);
    assert!(all.next_cursor.is_none());
}

#[test]
fn interleaved_non_matches_do_not_break_paging() {
    let mut store = Store::open_in_memory().unwrap();
    store.insert(&text_clip("M1", 500, "命中A")).unwrap();
    store.insert(&text_clip("N1", 400, "无关")).unwrap();
    store.insert(&text_clip("M2", 300, "命中B")).unwrap();
    store.insert(&text_clip("N2", 200, "无关x")).unwrap();
    store.insert(&text_clip("M3", 100, "命中C")).unwrap();
    let mut seen = Vec::new();
    let mut cursor: Option<ListCursor> = None;
    loop {
        let page = search_at(&store, "命中", 1, cursor.clone());
        if page.summaries.is_empty() {
            assert!(page.next_cursor.is_none());
            break;
        }
        seen.push(page.summaries[0].preview.clone());
        cursor = page.next_cursor.clone();
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(seen, vec!["命中A", "命中B", "命中C"]);
}

#[test]
fn deleted_samples_leave_search_and_cursor_resumes_keyset() {
    let mut store = Store::open_in_memory().unwrap();
    let first_saved = store.insert(&text_clip("M1", 300, "命中A")).unwrap();
    store.insert(&text_clip("M2", 200, "命中B")).unwrap();
    store.insert(&text_clip("M3", 100, "命中C")).unwrap();
    let first = search(&store, "命中", 1);
    assert_eq!(previews(&first), vec!["命中A"]);
    let cursor = first.next_cursor.clone().unwrap();
    assert!(store.delete(first_saved.clip_id).unwrap());
    // The cursor's last sort key is now deleted; keyset continuation still works.
    let second = search_at(&store, "命中", 1, Some(cursor));
    assert_eq!(previews(&second), vec!["命中B"]);
    let third = search_at(&store, "命中", 1, second.next_cursor.clone());
    assert_eq!(previews(&third), vec!["命中C"]);
    assert!(third.next_cursor.is_none());
    assert_eq!(previews(&search(&store, "命中", 50)), vec!["命中B", "命中C"]);
    assert!(search(&store, "命中A", 50).summaries.is_empty());
}

#[test]
fn boundary_inputs_stay_invalid_input() {
    let mut store = Store::open_in_memory().unwrap();
    store.insert(&text_clip("M1", 300, "命中A")).unwrap();
    store.insert(&text_clip("M2", 200, "命中B")).unwrap();
    let good = search(&store, "命中", 1);
    let cursor = good.next_cursor.clone().unwrap();
    assert_eq!(cursor.normalized_query, "命中");
    // Cross-query cursor reuse is rejected at the request parser.
    let crossed =
        ListClipsRequest { request_id: "x".into(), query: "别的".into(), limit: 1, cursor: Some(cursor) };
    assert_eq!(store.list_recent(&crossed).unwrap_err().code, ErrorCode::InvalidInput);
    for (upper, last, at) in [
        ("0", "1", 1),
        ("-1", "1", 1),
        ("03", "1", 1),
        ("18446744073709551616", "1", 1),
        ("5", "0", 1),
        ("5", "abc", 1),
        ("5", "6", 1),
        ("5", "1", -1),
    ] {
        let bad = ListClipsRequest {
            request_id: "x".into(),
            query: "命中".into(),
            limit: 1,
            cursor: Some(ListCursor {
                normalized_query: "命中".into(),
                upper_id: upper.into(),
                last_captured_at_ms: at,
                last_id: last.into(),
            }),
        };
        assert_eq!(store.list_recent(&bad).unwrap_err().code, ErrorCode::InvalidInput, "{upper}/{last}/{at}");
    }
    for limit in [0, 101] {
        let bad = ListClipsRequest { request_id: "x".into(), query: "命中".into(), limit, cursor: None };
        assert_eq!(store.list_recent(&bad).unwrap_err().code, ErrorCode::InvalidInput, "limit {limit}");
    }
    for limit in [1, 100] {
        let ok = ListClipsRequest { request_id: "x".into(), query: "命中".into(), limit, cursor: None };
        assert!(store.list_recent(&ok).is_ok(), "limit {limit}");
    }
    let max = "x".repeat(256);
    let over = "x".repeat(257);
    assert!(parse_list_clips_request(&json!({"request_id": "x", "query": max, "limit": 1})).is_ok());
    assert_eq!(
        parse_list_clips_request(&json!({"request_id": "x", "query": over, "limit": 1})).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    let over_request = ListClipsRequest { request_id: "x".into(), query: over, limit: 1, cursor: None };
    assert_eq!(store.list_recent(&over_request).unwrap_err().code, ErrorCode::InvalidInput);
    let bad_id = ListClipsRequest { request_id: String::new(), query: "命中".into(), limit: 1, cursor: None };
    assert_eq!(store.list_recent(&bad_id).unwrap_err().code, ErrorCode::InvalidInput);
}

// ---- E08.2 get_clip_text UTF-8 frames ----

fn html_clip(id: &str, at: i64) -> Snapshot {
    Snapshot {
        capture_id: id.into(),
        captured_at_ms: at,
        source_process: None,
        source_confidence: SourceConfidence::Unknown,
        omitted_formats: vec![],
        payloads: vec![Payload { format_name: "HTML Format".into(), data: b"<b>no text</b>".to_vec() }],
    }
}

fn text_at(store: &Store, clip_id: &str, offset: i64) -> GetClipTextResult {
    let request =
        parse_get_clip_text_request(&json!({"request_id": "g", "clip_id": clip_id, "offset": offset})).unwrap();
    store.get_clip_text(&request).unwrap()
}

fn text_at_err(store: &Store, clip_id: &str, offset: i64) -> ErrorCode {
    let request =
        parse_get_clip_text_request(&json!({"request_id": "g", "clip_id": clip_id, "offset": offset})).unwrap();
    store.get_clip_text(&request).unwrap_err().code
}

fn direct_text_err(store: &Store, request: GetClipTextRequest) -> ErrorCode {
    store.get_clip_text(&request).unwrap_err().code
}

#[test]
fn get_clip_text_short_text_echoes_and_reads_suffix() {
    let mut store = Store::open_in_memory().unwrap();
    let saved = store.insert(&text_clip("S1", 1000, "短中文😀测试")).unwrap();
    let id = saved.clip_id.to_string();
    let request = parse_get_clip_text_request(&json!({"request_id": "g1", "clip_id": id, "offset": 0})).unwrap();
    let frame = store.get_clip_text(&request).unwrap();
    assert_eq!(frame.request_id, "g1");
    assert_eq!(frame.clip_id, id);
    assert_eq!(frame.text, "短中文😀测试");
    assert!(frame.next_offset.is_none());
    parse_get_clip_text_result(&serde_json::to_value(&frame).unwrap(), &request).unwrap();
    let mid = text_at(&store, &id, 3);
    assert_eq!(mid.text, "中文😀测试");
    assert!(mid.next_offset.is_none());
    let suffix = text_at(&store, &id, 9);
    assert_eq!(suffix.text, "😀测试");
    assert!(suffix.next_offset.is_none());
}

#[test]
fn get_clip_text_exact_frame_bytes_single_and_split() {
    let mut store = Store::open_in_memory().unwrap();
    let exact = "x".repeat(65536);
    let saved_a = store.insert(&text_clip("E1", 1000, &exact)).unwrap();
    let id_a = saved_a.clip_id.to_string();
    let frame = text_at(&store, &id_a, 0);
    assert_eq!(frame.text.len(), 65536);
    assert_eq!(frame.text, exact);
    assert!(frame.next_offset.is_none(), "exact 65536-byte EOF frame needs no empty follow-up");
    let tail = format!("{}中", "x".repeat(65536));
    let saved_b = store.insert(&text_clip("E2", 2000, &tail)).unwrap();
    let id_b = saved_b.clip_id.to_string();
    let first = text_at(&store, &id_b, 0);
    assert_eq!(first.text.len(), 65536);
    assert!(first.text.chars().all(|c| c == 'x'));
    assert_eq!(first.next_offset, Some(65536));
    let second = text_at(&store, &id_b, 65536);
    assert_eq!(second.text, "中");
    assert!(second.next_offset.is_none());
}

#[test]
fn get_clip_text_boundary_back_off_keeps_chinese_and_rejects_mid_char() {
    let mut store = Store::open_in_memory().unwrap();
    let body = format!("{}中😀b", "a".repeat(65535));
    let saved = store.insert(&text_clip("B1", 1000, &body)).unwrap();
    let id = saved.clip_id.to_string();
    let first = text_at(&store, &id, 0);
    assert_eq!(first.text, "a".repeat(65535));
    assert_eq!(first.text.len(), 65535);
    assert_eq!(first.next_offset, Some(65535));
    let second = text_at(&store, &id, 65535);
    assert_eq!(second.text, "中😀b");
    assert!(second.next_offset.is_none());
    assert_eq!(text_at_err(&store, &id, 65536), ErrorCode::InvalidInput, "65536 falls inside 中");
}

#[test]
fn get_clip_text_emoji_is_never_split() {
    let mut store = Store::open_in_memory().unwrap();
    let body = format!("{}😀后", "a".repeat(65533));
    let saved = store.insert(&text_clip("EM", 1000, &body)).unwrap();
    let id = saved.clip_id.to_string();
    let first = text_at(&store, &id, 0);
    assert_eq!(first.text, "a".repeat(65533));
    assert_eq!(first.text.len(), 65533);
    assert_eq!(first.next_offset, Some(65533));
    let second = text_at(&store, &id, 65533);
    assert_eq!(second.text, "😀后");
    assert!(second.next_offset.is_none());
    assert_eq!(text_at_err(&store, &id, 65534), ErrorCode::InvalidInput, "65534 falls inside the emoji");
}

#[test]
fn get_clip_text_multi_frame_reassembly_and_progress() {
    let mut store = Store::open_in_memory().unwrap();
    let body = format!("{}abc😀{}z", "中".repeat(30000), "q".repeat(70000));
    let saved = store.insert(&text_clip("MF", 1000, &body)).unwrap();
    let id = saved.clip_id.to_string();
    let mut offset = 0i64;
    let mut joined = String::new();
    let mut frames = 0u32;
    loop {
        frames += 1;
        assert!(frames <= 16, "frame loop must terminate");
        let frame = text_at(&store, &id, offset);
        assert!(frame.text.len() <= 65536);
        assert_eq!(frame.request_id, "g");
        assert_eq!(frame.clip_id, id);
        joined.push_str(&frame.text);
        match frame.next_offset {
            None => {
                assert_eq!(offset + frame.text.len() as i64, body.len() as i64);
                break;
            }
            Some(next) => {
                assert!(next > offset, "offset must strictly advance");
                assert_eq!(next, offset + frame.text.len() as i64);
                offset = next;
            }
        }
    }
    assert!(frames >= 3);
    assert_eq!(joined.len(), body.len());
    assert_eq!(joined, body);
}

#[test]
fn get_clip_text_empty_and_textless_samples_are_zero_length() {
    let mut store = Store::open_in_memory().unwrap();
    let empty = store.insert(&text_clip("E0", 1000, "")).unwrap();
    let png = store.insert(&png_clip("P0", 2000)).unwrap();
    let html = store.insert(&html_clip("H0", 3000)).unwrap();
    for clip_id in [empty.clip_id, png.clip_id, html.clip_id] {
        let id = clip_id.to_string();
        let frame = text_at(&store, &id, 0);
        assert_eq!(frame.text, "");
        assert!(frame.next_offset.is_none());
        assert_eq!(text_at_err(&store, &id, 1), ErrorCode::InvalidInput, "zero-length text rejects offset 1");
    }
}

#[test]
fn get_clip_text_missing_deleted_and_offset_bounds() {
    let mut store = Store::open_in_memory().unwrap();
    assert_eq!(text_at_err(&store, "1", 0), ErrorCode::NotFound, "missing row");
    let saved = store.insert(&text_clip("B2", 1000, "边界")).unwrap();
    let id = saved.clip_id.to_string();
    let eof = text_at(&store, &id, 6);
    assert_eq!(eof.text, "");
    assert!(eof.next_offset.is_none());
    assert_eq!(text_at_err(&store, &id, 7), ErrorCode::InvalidInput, "past end");
    assert_eq!(text_at_err(&store, &id, 1), ErrorCode::InvalidInput, "mid 边");
    assert!(store.delete(saved.clip_id).unwrap());
    assert_eq!(text_at_err(&store, &id, 0), ErrorCode::NotFound, "deleted row");
    assert_eq!(
        direct_text_err(&store, GetClipTextRequest { request_id: "g".into(), clip_id: "1".into(), offset: -1 }),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        direct_text_err(
            &store,
            GetClipTextRequest { request_id: "g".into(), clip_id: "1".into(), offset: 9_007_199_254_740_992 }
        ),
        ErrorCode::InvalidInput
    );
    for bad_id in ["0", "01", "abc", ""] {
        assert_eq!(
            direct_text_err(&store, GetClipTextRequest { request_id: "g".into(), clip_id: bad_id.into(), offset: 0 }),
            ErrorCode::InvalidInput,
            "clip_id {bad_id:?}"
        );
    }
    assert_eq!(
        direct_text_err(&store, GetClipTextRequest { request_id: String::new(), clip_id: "1".into(), offset: 0 }),
        ErrorCode::InvalidInput
    );
}
