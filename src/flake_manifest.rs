//! Narrow typed edit surface over `flake.nix`.
//!
//! Most component flakes pin sibling inputs through `flake.lock`; those
//! never need a `flake.nix` edit. Where a pin lives in the input URL itself
//! (`github:owner/repo/<rev-or-ref>`), this model rewrites exactly that URL
//! literal and nothing else: the URL is parsed into a typed
//! [`InputUrl`] (winnow), rewritten in-type, and substituted back at its
//! recorded span. The tool does not model Nix source beyond locating input
//! URL literals.

use crate::cargo_manifest::GitReference;
use crate::error::Error;
use crate::flake_lock::InputName;
use crate::types::ComponentName;

/// A `flake.nix` document with its located input URL literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlakeManifest {
    text: String,
    inputs: Vec<InputUrlOccurrence>,
}

impl FlakeManifest {
    /// Locate and parse every input URL literal in `text`.
    pub fn from_nix_text(text: &str) -> Result<Self, Error> {
        todo!("winnow scanner over inputs.<name>.url = \"...\" and <name>.url = \"...\" forms")
    }

    /// The inputs whose URL carries an explicit ref or rev segment — the
    /// only ones a bump must rewrite here rather than in the lock.
    pub fn pinned_inputs(&self) -> Vec<&InputUrlOccurrence> {
        todo!()
    }

    /// Rewrite the named input's URL segment to `reference` in-type,
    /// returning the previous URL. Fails if the input's URL carries no
    /// pin segment (those pins live in the lock).
    pub fn rewrite_pinned_input(
        &mut self,
        input: &InputName,
        reference: GitReference,
    ) -> Result<InputUrl, Error> {
        todo!()
    }

    /// The document text with all rewrites applied.
    pub fn to_nix_text(&self) -> String {
        todo!()
    }
}

/// One input URL literal found in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputUrlOccurrence {
    input: InputName,
    url: InputUrl,
    span: TextSpan,
}

impl InputUrlOccurrence {
    pub fn input(&self) -> &InputName {
        &self.input
    }

    pub fn url(&self) -> &InputUrl {
        &self.url
    }
}

/// A parsed flake input URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputUrl {
    /// `github:<owner>/<repo>` or `github:<owner>/<repo>/<rev-or-ref>`.
    GitHub {
        owner: String,
        repository: ComponentName,
        pin: GitHubPin,
    },
    /// Any other scheme; opaque to the tool and never rewritten.
    Other(String),
}

/// The trailing segment of a `github:` input URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubPin {
    /// No third segment: the pin lives in `flake.lock`.
    Unpinned,
    /// A third segment naming a branch, tag, or revision: the pin lives in
    /// the URL and must be rewritten on bump.
    Pinned(String),
}

/// A byte range of the original document text occupied by one URL literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSpan {
    pub start: usize,
    pub end: usize,
}
