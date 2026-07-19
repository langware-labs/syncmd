//! Golden serialization tests: the JSON shape MUST match flow-sdk record conventions —
//! `type` first, `id` second (omitted when null), snake_case fields, `None` omitted.

use syncmd_core::model::{Asset, AssetMount, AssetType, MountType, Oid, Transform};
use syncmd_core::report::{
    GroupReport, GroupStatus, MountReport, ReportType, Summary, SyncReport,
};
use syncmd_core::WinnerReason;

/// Top-level keys in the order they appear in the raw JSON string (serde_json emits struct
/// fields in declaration order; we read that order off the string, not via Value which sorts).
fn top_keys(json: &str) -> Vec<String> {
    // Only the top-level object: stop at the first nested `[` or `{` after the opening brace.
    let inner = &json[1..];
    let mut keys = Vec::new();
    let mut depth = 0usize;
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            b'"' if depth == 0 => {
                // find closing quote
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'"' {
                    j += 1;
                }
                let key = &inner[start..j];
                // a key is followed by a colon
                if inner[j + 1..].trim_start().starts_with(':') {
                    keys.push(key.to_string());
                }
                i = j;
            }
            _ => {}
        }
        i += 1;
    }
    keys
}

/// Returns the byte offset of a top-level key as it appears in the raw JSON string.
fn key_pos(json: &str, key: &str) -> usize {
    json.find(&format!("\"{key}\"")).expect("key present")
}

#[test]
fn asset_mount_shape() {
    let m = AssetMount {
        type_: MountType::AssetMount,
        id: Some("11111111-1111-5111-8111-111111111111".into()),
        path: "CLAUDE.md".into(),
        harness: "claude".into(),
        oid: Some(Oid::new("deadbeef")),
        transform: Transform::Identity,
    };
    let json = serde_json::to_string(&m).unwrap();

    // type first, then id.
    assert!(key_pos(&json, "type") < key_pos(&json, "id"));
    assert!(key_pos(&json, "id") < key_pos(&json, "path"));
    // identity transform is omitted (skip_serializing_if).
    assert!(!json.contains("transform"), "identity transform must be omitted: {json}");
    // snake_case + values.
    assert!(json.contains("\"type\":\"asset_mount\""));
    assert!(json.contains("\"harness\":\"claude\""));
}

#[test]
fn asset_mount_omits_null_id_and_oid() {
    let m = AssetMount::new("GEMINI.md", "gemini"); // id=None, oid=None
    let json = serde_json::to_string(&m).unwrap();
    assert!(!json.contains("\"id\""), "null id must be omitted: {json}");
    assert!(!json.contains("\"oid\""), "null oid must be omitted: {json}");
    assert!(json.contains("\"path\":\"GEMINI.md\""));
}

#[test]
fn asset_shape() {
    let asset = Asset {
        type_: AssetType::ClaudeMd,
        id: "abc".into(),
        name: "instructions".into(),
        version: None,
        mounts: vec![AssetMount::new("CLAUDE.md", "claude")],
    };
    let json = serde_json::to_string(&asset).unwrap();
    assert!(key_pos(&json, "type") < key_pos(&json, "id"));
    assert!(json.contains("\"type\":\"claude_md\""));
    assert!(!json.contains("version"), "null version omitted: {json}");
    assert_eq!(
        top_keys(&json),
        vec![
            "type".to_string(),
            "id".to_string(),
            "name".to_string(),
            "mounts".to_string()
        ]
    );
}

#[test]
fn sync_report_shape() {
    let report = SyncReport {
        type_: ReportType::SyncReport,
        id: None,
        root: "/repo".into(),
        groups: vec![GroupReport {
            type_: AssetType::Skill,
            id: "skill-id".into(),
            name: "foo".into(),
            status: GroupStatus::Propagated,
            baseline: Some("base-oid".into()),
            winner_path: Some(".claude/skills/foo/SKILL.md".into()),
            winner_reason: Some(WinnerReason::SingleChange),
            winner_oid: Some("win-oid".into()),
            overridden: vec![],
            note: None,
            mounts: vec![MountReport {
                path: ".agents/skills/foo/SKILL.md".into(),
                action: "write".into(),
                from_oid: Some("win-oid".into()),
                applied: true,
            }],
        }],
        summary: Summary {
            groups: 1,
            propagated: 1,
            ..Default::default()
        },
    };
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("\"type\":\"sync_report\""));
    // top-level null id omitted; empty overridden omitted; null note omitted.
    assert!(!json.contains("\"note\""));
    assert!(!json.contains("\"overridden\""));
    // round-trips.
    let back: SyncReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}
