//! Stable cross-repository contract for Lean semantic search.
//!
//! The contract describes semantic facts and capability envelopes without
//! exposing expression traversal, feature-key encoding, storage layout, or
//! downstream report policy. Opaque keys are equality tokens: callers may
//! store and compare them under their matching version fields, but must not
//! interpret their bytes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version of the shared capability envelope used by this foundation crate.
pub const CAPABILITY_SCHEMA_VERSION: &str = "lean-semantic-search.capability.v1";

/// Version marker for foundation declaration feature command responses.
pub const DECLARATION_FEATURE_COMMAND_VERSION: &str = "declaration_features.foundation.v1";

/// Version marker for foundation proof-goal feature command responses.
pub const PROOF_GOAL_FEATURE_COMMAND_VERSION: &str = "proof_goal_features.foundation.v1";

/// Version marker for the empty foundation feature algorithm.
pub const FOUNDATION_FEATURE_VERSION: &str = "features.foundation.v1";

/// One command advertised through generic worker capability metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommandMetadata {
    /// Downstream-owned command name.
    pub name: String,
    /// Version of that command's request/response schema.
    pub version: String,
}

impl CommandMetadata {
    /// Build command metadata with owned strings.
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// One named capability advertised through generic worker capability metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityFact {
    /// Stable capability name.
    pub name: String,
    /// Version of the named capability.
    pub version: String,
}

impl CapabilityFact {
    /// Build capability metadata with owned strings.
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// Generic metadata shape transported by `lean-rs-worker`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityMetadata {
    /// Commands implemented by the loaded Lean package.
    pub commands: Vec<CommandMetadata>,
    /// Capability facts implemented by the loaded Lean package.
    pub capabilities: Vec<CapabilityFact>,
    /// Lean version reported by the package when available.
    pub lean_version: Option<String>,
    /// Downstream-owned metadata that does not affect the worker substrate.
    pub extra: Option<Value>,
}

/// Severity for both doctor diagnostics and feature-command diagnostics.
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

/// Generic doctor report shape transported by `lean-rs-worker`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DoctorReport {
    /// Structured package health diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// Optional package-owned metadata.
    pub metadata: Option<Value>,
}

/// One Lake module request target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModuleSpec {
    /// Dotted Lean module name.
    pub module: String,
    /// Optional caller-defined origin label.
    pub origin: Option<String>,
    /// Optional source root for later source-aware feature extraction.
    pub source_root: Option<String>,
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
    /// Optional declaration identifiers to extract. Empty means all eligible declarations.
    pub declaration_ids: Vec<String>,
}

/// Request for proof-goal feature extraction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProofGoalFeatureRequest {
    /// Module containing the proof state.
    pub module: String,
    /// Optional declaration name containing the proof state.
    pub declaration: Option<String>,
    /// Optional proof-state position.
    pub position: Option<SourcePosition>,
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

impl<Row> CommandResponse<Row> {
    /// Build an empty response for foundation commands.
    #[must_use]
    pub fn empty(
        command: impl Into<String>,
        command_version: impl Into<String>,
        feature_version: impl Into<String>,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            schema_version: CAPABILITY_SCHEMA_VERSION.to_owned(),
            command: command.into(),
            command_version: command_version.into(),
            feature_version: feature_version.into(),
            rows: Vec::new(),
            diagnostics,
        }
    }
}

/// Terminal metadata for the optional streaming declaration-feature export.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StreamSummary {
    /// Shared response envelope version.
    pub schema_version: String,
    /// Command name that produced the stream.
    pub command: String,
    /// Version of this command's request/response schema.
    pub command_version: String,
    /// Feature algorithm version represented by the stream.
    pub feature_version: String,
    /// Number of rows emitted by the command.
    pub rows: u64,
    /// Structured terminal diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json, to_value};

    use super::{CapabilityFact, CapabilityMetadata, CommandMetadata, Diagnostic, DiagnosticSeverity, DoctorReport};

    #[test]
    fn metadata_serializes_to_generic_worker_shape() -> Result<(), String> {
        let metadata = CapabilityMetadata {
            commands: vec![CommandMetadata::new("metadata", "v1")],
            capabilities: vec![CapabilityFact::new("rows.json", "v1")],
            lean_version: Some("lean-4".to_owned()),
            extra: Some(json!({ "schema_version": "test" })),
        };

        let value = to_value(metadata).map_err(|error| error.to_string())?;
        let first_command = first_object(&value, "commands")?;
        let first_capability = first_object(&value, "capabilities")?;

        assert_eq!(first_command.get("name").and_then(Value::as_str), Some("metadata"));
        assert_eq!(first_command.get("version").and_then(Value::as_str), Some("v1"));
        assert_eq!(first_capability.get("name").and_then(Value::as_str), Some("rows.json"));
        assert_eq!(value.get("lean_version").and_then(Value::as_str), Some("lean-4"));
        assert_eq!(
            value
                .get("extra")
                .and_then(|extra| extra.get("schema_version"))
                .and_then(Value::as_str),
            Some("test")
        );
        Ok(())
    }

    #[test]
    fn doctor_serializes_to_generic_worker_shape() -> Result<(), String> {
        let report = DoctorReport {
            diagnostics: vec![Diagnostic::new(
                DiagnosticSeverity::Pass,
                "foundation.ok",
                "foundation ready",
                Some(json!({ "check": "boundary" })),
            )],
            metadata: Some(json!({ "schema_version": "test" })),
        };

        let value = to_value(report).map_err(|error| error.to_string())?;
        let first_diagnostic = first_object(&value, "diagnostics")?;

        assert_eq!(first_diagnostic.get("severity").and_then(Value::as_str), Some("pass"));
        assert_eq!(
            first_diagnostic.get("code").and_then(Value::as_str),
            Some("foundation.ok")
        );
        assert_eq!(
            first_diagnostic
                .get("details")
                .and_then(|details| details.get("check"))
                .and_then(Value::as_str),
            Some("boundary")
        );
        assert_eq!(
            value
                .get("metadata")
                .and_then(|metadata| metadata.get("schema_version"))
                .and_then(Value::as_str),
            Some("test")
        );
        Ok(())
    }

    fn first_object<'value>(value: &'value Value, field: &str) -> Result<&'value Value, String> {
        value
            .get(field)
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .ok_or_else(|| format!("missing first object in `{field}`"))
    }
}
