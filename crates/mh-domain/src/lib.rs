//! Domain data structures shared by the host, database ingestor, and plugins.
//!
//! [`SourceRecord`] mirrors `docs/protocol-v1.md` §6 for
//! `record_schema_version = 1`.

use serde::{Deserialize, Serialize};

/// A publication metadata record emitted by a source plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRecord {
    pub source_name: String,
    pub source_url: String,
    pub title: String,
    pub brand_raw: String,
    #[serde(default)]
    pub performers_raw: Vec<String>,
    #[serde(default)]
    pub cover_urls: Vec<String>,
    #[serde(default)]
    pub page_urls: Vec<String>,
    #[serde(default)]
    pub external_links: Vec<ExternalLink>,
    #[serde(default)]
    pub issue_no: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub post_date: Option<String>,
    #[serde(default)]
    pub brand_normalized: Option<String>,
    #[serde(default)]
    pub normalizer_id: Option<String>,
    #[serde(default)]
    pub normalizer_version: Option<String>,
    #[serde(default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SourceRecord {
    /// Validate generic host-side invariants from the v1 contract.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_required("source_name", &self.source_name)?;
        validate_required("source_url", &self.source_url)?;
        validate_required("title", &self.title)?;
        validate_absolute_uri("source_url", &self.source_url)?;
        validate_list("performers_raw", &self.performers_raw, false)?;
        validate_list("cover_urls", &self.cover_urls, true)?;
        validate_list("page_urls", &self.page_urls, true)?;
        for (index, link) in self.external_links.iter().enumerate() {
            link.validate(index)?;
        }
        validate_date("release_date", self.release_date.as_deref())?;
        validate_date("post_date", self.post_date.as_deref())?;
        Ok(())
    }

    /// Validate this record against the source declared by a plugin manifest.
    pub fn validate_for_source(&self, source_name: &str) -> Result<(), ValidationError> {
        self.validate()?;
        if self.source_name != source_name {
            return Err(ValidationError::SourceNameMismatch {
                expected: source_name.to_string(),
                actual: self.source_name.clone(),
            });
        }
        Ok(())
    }
}

/// A typed external link attached to a source post.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalLink {
    pub url: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl ExternalLink {
    fn validate(&self, index: usize) -> Result<(), ValidationError> {
        validate_absolute_uri("external_links.url", &self.url)?;
        validate_optional_string("external_links.provider", index, self.provider.as_deref())?;
        validate_optional_string("external_links.label", index, self.label.as_deref())?;
        validate_optional_string("external_links.kind", index, self.kind.as_deref())?;
        validate_optional_string(
            "external_links.external_id",
            index,
            self.external_id.as_deref(),
        )?;
        Ok(())
    }
}

/// Validation errors for generic `SourceRecord` contract checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    MissingField { field: &'static str },
    InvalidUri { field: &'static str, value: String },
    EmptyListItem { field: &'static str, index: usize },
    InvalidDate { field: &'static str, value: String },
    EmptyOptionalField { field: &'static str, index: usize },
    SourceNameMismatch { expected: String, actual: String },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::MissingField { field } => {
                write!(f, "missing required field: {field}")
            }
            ValidationError::InvalidUri { field, value } => {
                write!(f, "invalid absolute URI in {field}: {value}")
            }
            ValidationError::EmptyListItem { field, index } => {
                write!(f, "empty item in {field}[{index}]")
            }
            ValidationError::InvalidDate { field, value } => {
                write!(f, "invalid YYYY-MM-DD date in {field}: {value}")
            }
            ValidationError::EmptyOptionalField { field, index } => {
                write!(f, "empty optional field in {field}[{index}]")
            }
            ValidationError::SourceNameMismatch { expected, actual } => {
                write!(f, "source_name mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

fn validate_required(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(ValidationError::MissingField { field })
    } else {
        Ok(())
    }
}

fn validate_list(
    field: &'static str,
    values: &[String],
    require_uri: bool,
) -> Result<(), ValidationError> {
    for (index, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            return Err(ValidationError::EmptyListItem { field, index });
        }
        if require_uri {
            validate_absolute_uri(field, value)?;
        }
    }
    Ok(())
}

fn validate_optional_string(
    field: &'static str,
    index: usize,
    value: Option<&str>,
) -> Result<(), ValidationError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        Err(ValidationError::EmptyOptionalField { field, index })
    } else {
        Ok(())
    }
}

fn validate_absolute_uri(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if is_absolute_uri(value) {
        Ok(())
    } else {
        Err(ValidationError::InvalidUri {
            field,
            value: value.to_string(),
        })
    }
}

fn is_absolute_uri(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    if colon == 0 {
        return false;
    }
    let scheme = &value[..colon];
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return false;
    }
    if scheme.contains(['/', '?', '#']) {
        return false;
    }
    !value[colon + 1..].is_empty() && !value.chars().any(char::is_whitespace)
}

fn validate_date(field: &'static str, value: Option<&str>) -> Result<(), ValidationError> {
    let Some(value) = value else {
        return Ok(());
    };
    if is_valid_yyyy_mm_dd(value) {
        Ok(())
    } else {
        Err(ValidationError::InvalidDate {
            field,
            value: value.to_string(),
        })
    }
}

fn is_valid_yyyy_mm_dd(value: &str) -> bool {
    if value.len() != 10 {
        return false;
    }
    let bytes = value.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit())
    {
        return false;
    }
    let year = value[0..4].parse::<u16>().ok();
    let month = value[5..7].parse::<u8>().ok();
    let day = value[8..10].parse::<u8>().ok();
    match (year, month, day) {
        (Some(year), Some(month @ 1..=12), Some(day)) => {
            day >= 1 && day <= days_in_month(year, month)
        }
        _ => false,
    }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> SourceRecord {
        SourceRecord {
            source_name: "synthetic".to_string(),
            source_url: "synthetic://post/1".to_string(),
            title: "Example title".to_string(),
            brand_raw: "Example brand".to_string(),
            performers_raw: vec!["Alice".to_string(), "Bob".to_string()],
            cover_urls: vec!["https://example.test/cover.jpg".to_string()],
            page_urls: vec!["https://example.test/page/1".to_string()],
            external_links: vec![ExternalLink {
                url: "https://retail.example.test/item/1".to_string(),
                provider: Some("retail".to_string()),
                label: None,
                kind: Some("retail".to_string()),
                external_id: Some("X1".to_string()),
                metadata: serde_json::Map::new(),
            }],
            issue_no: Some("42".to_string()),
            release_date: Some("2026-06-25".to_string()),
            post_date: None,
            brand_normalized: Some("Example Brand".to_string()),
            normalizer_id: Some("example-normalizer".to_string()),
            normalizer_version: Some("1.0.0".to_string()),
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn source_record_accepts_contract_shape() {
        let record = record();
        record.validate_for_source("synthetic").unwrap();
        let encoded = serde_json::to_value(&record).unwrap();
        assert_eq!(encoded["issue_no"], "42");
        assert_eq!(encoded["page_urls"][0], "https://example.test/page/1");
        assert_eq!(encoded["external_links"][0]["external_id"], "X1");
    }

    #[test]
    fn rejects_relative_source_url() {
        let mut record = record();
        record.source_url = "/post/1".to_string();
        assert!(matches!(
            record.validate(),
            Err(ValidationError::InvalidUri {
                field: "source_url",
                ..
            })
        ));
    }

    #[test]
    fn rejects_invalid_dates() {
        let mut record = record();
        record.release_date = Some("2026-02-29".to_string());
        assert!(matches!(
            record.validate(),
            Err(ValidationError::InvalidDate {
                field: "release_date",
                ..
            })
        ));
    }

    #[test]
    fn rejects_relative_external_link_url() {
        let mut record = record();
        record.external_links[0].url = "relative".to_string();
        assert!(matches!(
            record.validate(),
            Err(ValidationError::InvalidUri {
                field: "external_links.url",
                ..
            })
        ));
    }
}
