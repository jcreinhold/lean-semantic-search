//! Stable cross-repository contract for Lean semantic search.
//!
//! The contract describes semantic facts and command envelopes without
//! exposing expression traversal, feature-key encoding, storage layout, or
//! downstream report policy. Opaque keys are equality tokens: callers may
//! store and compare them under their matching version fields, but must not
//! interpret their bytes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version of the command response envelope.
pub const CAPABILITY_SCHEMA_VERSION: &str = "lean-semantic-search.capability.v1";

/// Name of the declaration feature command, as its responses carry it.
pub const DECLARATION_FEATURES_COMMAND: &str = "declaration_features";

/// Version marker for declaration feature command responses.
pub const DECLARATION_FEATURE_COMMAND_VERSION: &str = "declaration_features.v1";

/// Name of the proof-goal feature command, as its responses carry it.
pub const PROOF_GOAL_FEATURES_COMMAND: &str = "proof_goal_features";

/// Version marker for proof-goal feature command responses.
pub const PROOF_GOAL_FEATURE_COMMAND_VERSION: &str = "proof_goal_features.v1";

/// Version marker for canonical Lean expression fingerprints.
pub const CANONICAL_FEATURE_VERSION: &str = "canonical.expr.v3";

/// Version marker for semantic role-feature rows.
pub const SEMANTIC_FEATURE_VERSION: &str = "features.roles.v3";

/// Severity of a command or retrieval diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    /// A check passed.
    Pass,
    /// A check found a non-blocking issue.
    Warning,
    /// A check found a blocking issue.
    Error,
}

/// One bounded diagnostic emitted by the shared search package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    /// Diagnostic severity.
    pub severity: DiagnosticSeverity,
    /// Stable machine-readable diagnostic code.
    pub code: String,
    /// Bounded human-readable diagnostic message.
    pub message: String,
    /// Optional structured details.
    pub details: Option<Value>,
}

impl Diagnostic {
    /// Build a diagnostic with optional structured details.
    #[must_use]
    pub fn new(
        severity: DiagnosticSeverity,
        code: impl Into<String>,
        message: impl Into<String>,
        details: Option<Value>,
    ) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            details,
        }
    }
}

/// One Lake module request target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModuleSpec {
    /// Dotted Lean module name.
    pub module: String,
    /// Optional caller-defined origin label.
    pub origin: Option<String>,
}

/// 1-based source position used to identify a proof state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourcePosition {
    /// 1-based line number.
    pub line: u32,
    /// 1-based UTF-8 column number.
    pub column: u32,
}

/// 1-based inclusive source span.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceSpan {
    /// Span start position.
    pub start: SourcePosition,
    /// Span end position.
    pub end: SourcePosition,
}

/// Request for declaration feature extraction.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeclarationFeatureRequest {
    /// Modules whose imported environments should be available to extraction.
    pub modules: Vec<ModuleSpec>,
    /// Optional declaration identifiers to extract.
    ///
    /// An empty vector omits the wire selector and discovers all eligible declarations.
    /// Explicit JSON `[]` selects none; this discovery-oriented DTO does not emit it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declaration_ids: Vec<String>,
}

/// Request for proof-goal feature extraction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProofGoalFeatureRequest {
    /// Module containing the proof state.
    pub module: String,
    /// Full Lean source text to elaborate before selecting the proof state.
    pub source_text: String,
    /// Optional file label used for Lean diagnostics.
    pub file_label: Option<String>,
    /// Optional declaration name containing the proof state.
    pub declaration: Option<String>,
    /// Optional proof-state position.
    pub position: Option<SourcePosition>,
    /// Optional namespace context for source snippets that omit their namespace wrapper.
    pub namespace: Option<String>,
}

/// An opaque semantic equality key.
#[derive(Clone, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct OpaqueFeatureKey(String);

impl OpaqueFeatureKey {
    /// Build an opaque key from a package-owned encoded value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the encoded key for equality storage or comparison.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Opaque fingerprints for one declaration statement or proof goal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Fingerprints {
    /// Full statement fingerprint.
    pub statement: OpaqueFeatureKey,
    /// Binder-permutation-safe fingerprint.
    pub safe_binder_permutation: OpaqueFeatureKey,
    /// Connective-normalized statement fingerprint.
    pub connective_shape: OpaqueFeatureKey,
    /// Connective-normalized conclusion fingerprint.
    pub conclusion_shape: OpaqueFeatureKey,
}

/// One role-aware semantic feature.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RoleFeature {
    /// Stable role label such as `conclusion_const`.
    pub role: String,
    /// Opaque equality key owned by the Lean feature package.
    pub key: OpaqueFeatureKey,
    /// Optional display label for diagnostics only.
    pub display: Option<String>,
}

/// One declaration feature row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeatureRow {
    /// Stable declaration identifier supplied by the extractor.
    pub declaration_id: String,
    /// Feature algorithm version for this row.
    pub feature_version: String,
    /// Opaque declaration fingerprints.
    pub fingerprints: Fingerprints,
    /// Role-aware semantic features.
    pub role_features: Vec<RoleFeature>,
    /// Number of top-level binders observed by the Lean extractor.
    pub binder_count: u32,
    /// Stable low-signal markers used by retrieval policy.
    pub low_signal_markers: Vec<String>,
    /// Optional source span for diagnostics and caller correlation.
    pub source: Option<SourceSpan>,
}

/// Declaration-feature rows use the generic feature-row shape.
pub type DeclarationFeatureRow = FeatureRow;

/// One proof-goal feature row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProofGoalFeatureRow {
    /// Stable goal identifier within the request.
    pub goal_id: String,
    /// Feature algorithm version for this row.
    pub feature_version: String,
    /// Opaque proof-goal fingerprints.
    pub fingerprints: Fingerprints,
    /// Role-aware proof-goal semantic features.
    pub role_features: Vec<RoleFeature>,
    /// Stable low-signal markers used by retrieval policy.
    pub low_signal_markers: Vec<String>,
}

/// Generic response for JSON feature commands.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommandResponse<Row> {
    /// Shared response envelope version.
    pub schema_version: String,
    /// Command name that produced this response.
    pub command: String,
    /// Version of this command's request/response schema.
    pub command_version: String,
    /// Feature algorithm version represented by the rows.
    pub feature_version: String,
    /// Extracted feature rows.
    pub rows: Vec<Row>,
    /// Structured command diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(test)]
mod tests {
    use serde_json::{json, to_value};

    use super::{DeclarationFeatureRequest, ModuleSpec};

    fn declaration_request() -> DeclarationFeatureRequest {
        DeclarationFeatureRequest {
            modules: vec![ModuleSpec {
                module: "Example.Basic".to_owned(),
                origin: None,
            }],
            declaration_ids: Vec::new(),
        }
    }

    #[test]
    fn declaration_discovery_omits_selector_and_accepts_omission() -> serde_json::Result<()> {
        let request = declaration_request();
        let value = to_value(&request)?;
        assert!(
            value.get("declaration_ids").is_none(),
            "discovery must omit the selector; [] selects no declarations"
        );
        let decoded: DeclarationFeatureRequest = serde_json::from_value(value)?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn declaration_selection_preserves_nonempty_ids() -> serde_json::Result<()> {
        let mut request = declaration_request();
        request
            .declaration_ids
            .push("workspace:Example.Basic:Example.target".to_owned());
        let value = to_value(&request)?;
        assert_eq!(
            value.get("declaration_ids"),
            Some(&json!(["workspace:Example.Basic:Example.target"]))
        );
        let decoded: DeclarationFeatureRequest = serde_json::from_value(value)?;
        assert_eq!(decoded, request);
        Ok(())
    }
}
