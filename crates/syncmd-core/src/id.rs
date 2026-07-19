//! Stable id minting — must produce byte-identical uuids to flow-sdk's Python `uuid.uuid5`.
//!
//! Per-type keying (verified against flow-sdk source):
//! * skill / agent → `uuid5(NAMESPACE_DNS, "<type>:<name>")`
//! * markdown / spec / claude_md → `uuid5(NAMESPACE_URL, <resolved_path>)` (bare path, no
//!   `"<type>:"` prefix).

use uuid::Uuid;

use crate::model::{AssetType, IdKey};

/// Mint the stable asset id for `ty`, using `name` or `resolved_path` per the type's key.
pub fn mint_id(ty: AssetType, name: &str, resolved_path: &str) -> String {
    match ty.id_key() {
        IdKey::Name => {
            let key = format!("{}:{}", ty.as_str(), name);
            Uuid::new_v5(&Uuid::NAMESPACE_DNS, key.as_bytes()).to_string()
        }
        IdKey::ResolvedPath => {
            Uuid::new_v5(&Uuid::NAMESPACE_URL, resolved_path.as_bytes()).to_string()
        }
    }
}

/// Mint a mount id, keyed on `"<harness>:<path>"` under the URL namespace. Deterministic so a
/// mount's id is stable across runs.
pub fn mint_mount_id(harness: &str, path: &str) -> String {
    let key = format!("{harness}:{path}");
    Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()).to_string()
}

/// The flow-sdk TypeId string form: `"<type>-<uuid>"` (single dash; the uuid keeps its dashes,
/// so consumers split on the *first* dash).
pub fn type_id(ty: AssetType, id: &str) -> String {
    format!("{}-{}", ty.as_str(), id)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden vectors captured from Python `uuid.uuid5(...)` (flow-sdk's minter). If these ever
    // fail, Rust and flow-sdk have drifted — a hard contract break.
    //   uuid5(NAMESPACE_DNS, "skill:foo")        = 2c49e5e4-9c5b-5975-845f-dbcc8dcc02af
    //   uuid5(NAMESPACE_DNS, "agent:reviewer")   = 5186e5a2-add3-5b1b-9be0-594172c05bb2
    //   uuid5(NAMESPACE_URL, "/repo/docs/x.md")  = 6ffdd2a9-f707-505c-bd09-892d3fed0119
    //   uuid5(NAMESPACE_URL, "claude:CLAUDE.md") = f8b1cf9f-013a-5c4f-8d1b-bf1cffda2221

    #[test]
    fn golden_vectors_match_python_uuid5() {
        assert_eq!(
            mint_id(AssetType::Skill, "foo", "/ignored"),
            "2c49e5e4-9c5b-5975-845f-dbcc8dcc02af"
        );
        assert_eq!(
            mint_id(AssetType::Agent, "reviewer", "/ignored"),
            "5186e5a2-add3-5b1b-9be0-594172c05bb2"
        );
        assert_eq!(
            mint_id(AssetType::Markdown, "ignored", "/repo/docs/x.md"),
            "6ffdd2a9-f707-505c-bd09-892d3fed0119"
        );
        assert_eq!(
            mint_mount_id("claude", "CLAUDE.md"),
            "f8b1cf9f-013a-5c4f-8d1b-bf1cffda2221"
        );
    }

    #[test]
    fn deterministic_and_namespace_correct() {
        // Determinism: same inputs → same id.
        let a = mint_id(AssetType::Skill, "foo", "");
        let b = mint_id(AssetType::Skill, "foo", "");
        assert_eq!(a, b);

        // Skill keys on name under DNS namespace.
        let expected_skill =
            Uuid::new_v5(&Uuid::NAMESPACE_DNS, b"skill:foo").to_string();
        assert_eq!(mint_id(AssetType::Skill, "foo", "/ignored"), expected_skill);

        // Markdown keys on the bare resolved path under URL namespace (no "markdown:" prefix).
        let expected_md =
            Uuid::new_v5(&Uuid::NAMESPACE_URL, b"/repo/docs/x.md").to_string();
        assert_eq!(
            mint_id(AssetType::Markdown, "ignored", "/repo/docs/x.md"),
            expected_md
        );

        // TypeId form.
        assert_eq!(type_id(AssetType::Skill, &a), format!("skill-{a}"));
    }
}
