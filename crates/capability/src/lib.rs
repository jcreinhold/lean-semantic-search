//! Worker-facing capability boundary for Lean semantic search.
//!
//! This crate owns command identity, export names, metadata, and doctor
//! responses for the standalone capability. It deliberately does not own
//! retrieval policy: candidate generation and ranking belong in a future
//! `retrieval` crate once there is real functionality to hide.

use lean_semantic_search_contract::{
    CANONICAL_FEATURE_VERSION, CAPABILITY_SCHEMA_VERSION, CapabilityFact, CapabilityMetadata, CommandMetadata,
    CommandResponse, DECLARATION_FEATURE_COMMAND_VERSION, DeclarationFeatureRow, Diagnostic, DiagnosticSeverity,
    PROOF_GOAL_FEATURE_COMMAND_VERSION, ProofGoalFeatureRow, SEMANTIC_FEATURE_VERSION, StreamSummary,
};
use serde_json::json;

/// Stable metadata command name.
pub const METADATA_COMMAND: &str = "metadata";

/// Stable doctor command name.
pub const DOCTOR_COMMAND: &str = "doctor";

/// Stable declaration feature command name.
pub const DECLARATION_FEATURES_COMMAND: &str = "declaration_features";

/// Stable proof-goal feature command name.
pub const PROOF_GOAL_FEATURES_COMMAND: &str = "proof_goal_features";

/// Stable streaming declaration feature command name.
pub const STREAM_DECLARATION_FEATURES_COMMAND: &str = "stream_declaration_features";

/// Lean export for capability metadata.
pub const METADATA_EXPORT: &str = "lean_semantic_search_metadata";

/// Lean export for capability doctor diagnostics.
pub const DOCTOR_EXPORT: &str = "lean_semantic_search_doctor";

/// Lean export for declaration feature extraction.
pub const DECLARATION_FEATURES_EXPORT: &str = "lean_semantic_search_declaration_features";

/// Lean export for proof-goal feature extraction.
pub const PROOF_GOAL_FEATURES_EXPORT: &str = "lean_semantic_search_proof_goal_features";

/// Lean export for optional streaming declaration feature extraction.
pub const STREAM_DECLARATION_FEATURES_EXPORT: &str = "lean_semantic_search_stream_declaration_features";

/// Export names implemented by the Lean package.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportNames {
    /// Metadata export name.
    pub metadata: &'static str,
    /// Doctor export name.
    pub doctor: &'static str,
    /// Declaration-feature JSON command export name.
    pub declaration_features: &'static str,
    /// Proof-goal-feature JSON command export name.
    pub proof_goal_features: &'static str,
    /// Optional streaming declaration-feature export name.
    pub stream_declaration_features: &'static str,
}

/// Stable export names for this package.
pub const EXPORTS: ExportNames = ExportNames {
    metadata: METADATA_EXPORT,
    doctor: DOCTOR_EXPORT,
    declaration_features: DECLARATION_FEATURES_EXPORT,
    proof_goal_features: PROOF_GOAL_FEATURES_EXPORT,
    stream_declaration_features: STREAM_DECLARATION_FEATURES_EXPORT,
};

/// Command metadata advertised by the capability.
#[must_use]
pub fn command_metadata() -> Vec<CommandMetadata> {
    vec![
        CommandMetadata::new(METADATA_COMMAND, CAPABILITY_SCHEMA_VERSION),
        CommandMetadata::new(DOCTOR_COMMAND, CAPABILITY_SCHEMA_VERSION),
        CommandMetadata::new(DECLARATION_FEATURES_COMMAND, DECLARATION_FEATURE_COMMAND_VERSION),
        CommandMetadata::new(PROOF_GOAL_FEATURES_COMMAND, PROOF_GOAL_FEATURE_COMMAND_VERSION),
        CommandMetadata::new(STREAM_DECLARATION_FEATURES_COMMAND, DECLARATION_FEATURE_COMMAND_VERSION),
    ]
}

/// Capability metadata for the package.
#[must_use]
pub fn capability_metadata(lean_version: Option<String>) -> CapabilityMetadata {
    CapabilityMetadata {
        commands: command_metadata(),
        capabilities: vec![
            CapabilityFact::new("semantic_features.declarations", SEMANTIC_FEATURE_VERSION),
            CapabilityFact::new("semantic_features.proof_goals", SEMANTIC_FEATURE_VERSION),
            CapabilityFact::new("rows.json.streaming", CAPABILITY_SCHEMA_VERSION),
            CapabilityFact::new("diagnostics.structured", CAPABILITY_SCHEMA_VERSION),
        ],
        lean_version,
        extra: Some(json!({
            "schema_version": CAPABILITY_SCHEMA_VERSION,
            "package": "lean-semantic-search",
            "canonical_version": CANONICAL_FEATURE_VERSION,
            "feature_version": SEMANTIC_FEATURE_VERSION
        })),
    }
}

/// Doctor report for the package.
#[must_use]
pub fn doctor_report() -> lean_semantic_search_contract::DoctorReport {
    lean_semantic_search_contract::DoctorReport {
        diagnostics: vec![
            Diagnostic::new(
                DiagnosticSeverity::Pass,
                "lean_semantic_search.boundary.ready",
                "standalone semantic-search boundary is available",
                Some(json!({ "package": "lean-semantic-search" })),
            ),
            Diagnostic::new(
                DiagnosticSeverity::Pass,
                "lean_semantic_search.features.ready",
                "semantic feature extraction is available",
                Some(json!({
                    "canonical_version": CANONICAL_FEATURE_VERSION,
                    "feature_version": SEMANTIC_FEATURE_VERSION
                })),
            ),
        ],
        metadata: Some(json!({
            "schema_version": CAPABILITY_SCHEMA_VERSION,
            "canonical_version": CANONICAL_FEATURE_VERSION,
            "feature_version": SEMANTIC_FEATURE_VERSION
        })),
    }
}

/// Empty declaration-feature response with caller-supplied diagnostics.
#[must_use]
pub fn declaration_feature_empty_response(diagnostics: Vec<Diagnostic>) -> CommandResponse<DeclarationFeatureRow> {
    CommandResponse::empty(
        DECLARATION_FEATURES_COMMAND,
        DECLARATION_FEATURE_COMMAND_VERSION,
        SEMANTIC_FEATURE_VERSION,
        diagnostics,
    )
}

/// Empty proof-goal-feature response with caller-supplied diagnostics.
#[must_use]
pub fn proof_goal_feature_empty_response(diagnostics: Vec<Diagnostic>) -> CommandResponse<ProofGoalFeatureRow> {
    CommandResponse::empty(
        PROOF_GOAL_FEATURES_COMMAND,
        PROOF_GOAL_FEATURE_COMMAND_VERSION,
        SEMANTIC_FEATURE_VERSION,
        diagnostics,
    )
}

/// Empty terminal metadata for the optional streaming declaration-feature export.
#[must_use]
pub fn stream_declaration_feature_summary(diagnostics: Vec<Diagnostic>) -> StreamSummary {
    StreamSummary {
        schema_version: CAPABILITY_SCHEMA_VERSION.to_owned(),
        command: STREAM_DECLARATION_FEATURES_COMMAND.to_owned(),
        command_version: DECLARATION_FEATURE_COMMAND_VERSION.to_owned(),
        feature_version: SEMANTIC_FEATURE_VERSION.to_owned(),
        rows: 0,
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DECLARATION_FEATURES_COMMAND, DECLARATION_FEATURES_EXPORT, DOCTOR_EXPORT, EXPORTS, METADATA_EXPORT,
        PROOF_GOAL_FEATURES_COMMAND, PROOF_GOAL_FEATURES_EXPORT, STREAM_DECLARATION_FEATURES_EXPORT,
        capability_metadata, command_metadata, declaration_feature_empty_response, doctor_report,
        proof_goal_feature_empty_response,
    };

    #[test]
    fn export_names_match_capability_contract() {
        assert_eq!(METADATA_EXPORT, "lean_semantic_search_metadata");
        assert_eq!(DOCTOR_EXPORT, "lean_semantic_search_doctor");
        assert_eq!(DECLARATION_FEATURES_EXPORT, "lean_semantic_search_declaration_features");
        assert_eq!(PROOF_GOAL_FEATURES_EXPORT, "lean_semantic_search_proof_goal_features");
        assert_eq!(
            STREAM_DECLARATION_FEATURES_EXPORT,
            "lean_semantic_search_stream_declaration_features"
        );
        assert_eq!(EXPORTS.metadata, METADATA_EXPORT);
    }

    #[test]
    fn metadata_contains_versioned_capability_facts() {
        let metadata = capability_metadata(Some("lean-4".to_owned()));
        let command_names = metadata
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>();

        assert!(command_names.contains(&"metadata"));
        assert!(command_names.contains(&DECLARATION_FEATURES_COMMAND));
        assert!(command_names.contains(&PROOF_GOAL_FEATURES_COMMAND));
        assert_eq!(metadata.lean_version.as_deref(), Some("lean-4"));
        assert!(
            metadata
                .capabilities
                .iter()
                .any(|fact| fact.name == "semantic_features.declarations" && fact.version == "features.roles.v3")
        );
    }

    #[test]
    fn command_constants_are_advertised_by_metadata() {
        let worker_commands = command_metadata();
        let metadata = capability_metadata(None);

        assert!(
            worker_commands
                .iter()
                .any(|command| command.name == DECLARATION_FEATURES_COMMAND)
        );
        for command in worker_commands {
            assert!(
                metadata
                    .commands
                    .iter()
                    .any(|metadata_command| metadata_command.name == command.name
                        && metadata_command.version == command.version),
                "capability command metadata missing from metadata: {command:?}"
            );
        }
    }

    #[test]
    fn doctor_contains_boundary_and_feature_passes() {
        let report = doctor_report();
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "lean_semantic_search.boundary.ready")
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "lean_semantic_search.features.ready")
        );
    }

    #[test]
    fn capability_payloads_do_not_contain_downstream_policy_vocabulary() -> Result<(), serde_json::Error> {
        let payloads = [
            serde_json::to_string(&declaration_feature_empty_response(Vec::new()))?,
            serde_json::to_string(&proof_goal_feature_empty_response(Vec::new()))?,
            serde_json::to_string(&capability_metadata(None))?,
        ];

        for payload in payloads {
            // These name downstream retrieval, storage, and transport policy
            // (duplicate-review workflow, vector stores, MCP/HTTP, the actor
            // runtime). None of it may surface in the boundary contract.
            for forbidden in [
                "duplicate-audit",
                "replacement_hint",
                "baseline",
                "review_group",
                "sqlite",
                "vector",
                "embedding",
                "mcp",
                "http",
                "actor",
            ] {
                assert!(
                    !payload.to_ascii_lowercase().contains(forbidden),
                    "capability payload leaked forbidden vocabulary `{forbidden}`: {payload}"
                );
            }
        }
        Ok(())
    }
}
