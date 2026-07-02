//! Format-preserving TOML write surface.
//!
//! Pin writes are psyche-locked to be non-destructive: a bump must leave
//! every comment, alignment, and layout byte of a manifest untouched except
//! for the edited values (spirit's `Cargo.toml` carries load-bearing
//! comments). [`PreservedTomlDocument`] wraps `toml_edit::DocumentMut` — a
//! typed, format-preserving TOML model — and is the only way this crate
//! rewrites TOML. Serde models remain the read surface for topology and
//! staleness; this document is the write surface.

use toml_edit::DocumentMut;

use crate::error::Error;
use crate::topology::PinLayer;
use crate::types::{ComponentName, TomlText};

/// One TOML document held in its format-preserving representation.
#[derive(Debug, Clone)]
pub struct PreservedTomlDocument {
    document: DocumentMut,
}

impl PreservedTomlDocument {
    /// Parse `text` into the format-preserving representation.
    pub fn parse(text: &str, component: &ComponentName, layer: PinLayer) -> Result<Self, Error> {
        let document = text
            .parse::<DocumentMut>()
            .map_err(|error| Error::ManifestDecode {
                component: component.clone(),
                layer,
                detail: error.to_string(),
            })?;
        Ok(Self { document })
    }

    /// The typed mutable document. Every edit through this handle preserves
    /// the formatting of untouched nodes.
    pub fn as_document_mut(&mut self) -> &mut DocumentMut {
        &mut self.document
    }

    /// The typed read view of the document.
    pub fn as_document(&self) -> &DocumentMut {
        &self.document
    }

    /// The document text with all edits applied and all untouched
    /// formatting preserved.
    pub fn to_toml_text(&self) -> TomlText {
        TomlText::new(self.document.to_string())
    }
}
