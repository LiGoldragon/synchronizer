//! Narrow typed edit surface over `flake.nix`.
//!
//! Most component flakes pin sibling inputs through `flake.lock`; those
//! never need a `flake.nix` edit. Where a pin lives in the input URL itself
//! (`github:owner/repo/<rev-or-ref>`), this model rewrites exactly that URL
//! literal and nothing else: the URL is parsed into a typed [`InputUrl`]
//! (winnow), rewritten in-type, and substituted back at its recorded span.
//! The tool does not model Nix source beyond input URL literals; the
//! scanner recognizes the `inputs.<name>.url = "..."` and
//! `<name>.url = "..."` assignment forms. A URL authored inside a nested
//! attrset literal (`<name> = { url = "..."; }`) is outside the modeled
//! grammar; its pin lives in the lock, which is still repinned.

use winnow::Parser;
use winnow::combinator::{opt, separated};
use winnow::token::take_while;

use crate::cargo_manifest::GitReference;
use crate::error::Error;
use crate::flake_lock::InputName;
use crate::topology::PinLayer;
use crate::types::{CommitIdentifier, ComponentName};

/// A `flake.nix` document with its located input URL literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlakeManifest {
    text: String,
    inputs: Vec<InputUrlOccurrence>,
}

impl FlakeManifest {
    /// Locate and parse every input URL literal in `text`.
    pub fn from_nix_text(text: &str, component: &ComponentName) -> Result<Self, Error> {
        let mut inputs = Vec::new();
        let mut offset = 0;
        let mut remaining = text;
        while !remaining.is_empty() {
            let mut probe = remaining;
            let before = probe.len();
            match UrlAssignment::parse(&mut probe) {
                Ok(assignment) => {
                    let consumed = before - probe.len();
                    let literal_end = offset + consumed - 1; // closing quote
                    let literal_start = literal_end - assignment.literal.len();
                    let url = InputUrl::parse(&assignment.literal).map_err(|detail| {
                        Error::ManifestDecode {
                            component: component.clone(),
                            layer: PinLayer::FlakeManifest,
                            detail,
                        }
                    })?;
                    inputs.push(InputUrlOccurrence {
                        input: assignment.input,
                        url,
                        span: TextSpan {
                            start: literal_start,
                            end: literal_end,
                        },
                        edit_state: UrlEditState::Untouched,
                    });
                    offset += consumed;
                    remaining = probe;
                }
                Err(_) => {
                    let mut characters = remaining.chars();
                    let Some(character) = characters.next() else {
                        break;
                    };
                    offset += character.len_utf8();
                    remaining = characters.as_str();
                }
            }
        }
        Ok(Self {
            text: text.to_string(),
            inputs,
        })
    }

    /// Every located input URL.
    pub fn inputs(&self) -> &[InputUrlOccurrence] {
        &self.inputs
    }

    /// The inputs whose URL carries an explicit revision segment — the only
    /// ones a bump must rewrite here rather than in the lock.
    pub fn pinned_inputs(&self) -> Vec<&InputUrlOccurrence> {
        self.inputs
            .iter()
            .filter(|occurrence| occurrence.url.is_revision_pinned())
            .collect()
    }

    /// Rewrite the named input's URL pin segment to `reference` in-type,
    /// returning the previous URL. Fails if the input's URL carries no pin
    /// segment (those pins live in the lock).
    pub fn rewrite_pinned_input(
        &mut self,
        component: &ComponentName,
        input: &InputName,
        reference: GitReference,
    ) -> Result<InputUrl, Error> {
        let occurrence = self
            .inputs
            .iter_mut()
            .find(|occurrence| &occurrence.input == input)
            .ok_or_else(|| Error::ManifestEncode {
                component: component.clone(),
                layer: PinLayer::FlakeManifest,
                detail: format!("no input url literal named {}", input.as_str()),
            })?;
        let previous = occurrence.url.clone();
        let segment = match reference {
            GitReference::Revision(revision) => revision.as_str().to_string(),
            GitReference::Branch(branch) => branch.as_str().to_string(),
            other => {
                return Err(Error::ManifestEncode {
                    component: component.clone(),
                    layer: PinLayer::FlakeManifest,
                    detail: format!("unsupported url pin rewrite target: {other:?}"),
                });
            }
        };
        match &mut occurrence.url {
            InputUrl::GitHub { pin, .. } => match pin {
                GitHubPin::Pinned(existing) => {
                    *existing = segment;
                    occurrence.edit_state = UrlEditState::Rewritten;
                }
                GitHubPin::Unpinned => {
                    return Err(Error::ManifestEncode {
                        component: component.clone(),
                        layer: PinLayer::FlakeManifest,
                        detail: format!(
                            "input {} carries no url pin segment; its pin lives in the lock",
                            input.as_str()
                        ),
                    });
                }
            },
            InputUrl::Other(_) => {
                return Err(Error::ManifestEncode {
                    component: component.clone(),
                    layer: PinLayer::FlakeManifest,
                    detail: format!("input {} is not a github url", input.as_str()),
                });
            }
        }
        Ok(previous)
    }

    /// The document text with the rewritten literals substituted at their
    /// recorded spans; untouched literals keep their original bytes.
    pub fn to_nix_text(&self) -> String {
        let mut rendered = self.text.clone();
        let mut spans: Vec<(&InputUrlOccurrence, String)> = self
            .inputs
            .iter()
            .filter(|occurrence| occurrence.edit_state == UrlEditState::Rewritten)
            .map(|occurrence| (occurrence, occurrence.url.render()))
            .collect();
        spans.sort_by_key(|(occurrence, _)| std::cmp::Reverse(occurrence.span.start));
        for (occurrence, replacement) in spans {
            rendered.replace_range(occurrence.span.start..occurrence.span.end, &replacement);
        }
        rendered
    }
}

/// One parsed `<path>.url = "<literal>"` assignment.
#[derive(Debug)]
struct UrlAssignment {
    input: InputName,
    literal: String,
}

impl UrlAssignment {
    /// Winnow grammar anchored at the current position:
    /// `(inputs.)<name>(.<segment>)*.url = "<literal>"` where the final
    /// path segment before `url` names the input.
    fn parse(input: &mut &str) -> winnow::Result<Self> {
        let identifier = || {
            take_while(1.., |character: char| {
                character.is_ascii_alphanumeric() || character == '_' || character == '-'
            })
        };
        let path: Vec<&str> = separated(2.., identifier(), '.').parse_next(input)?;
        let _ = (
            take_while(0.., |character: char| character == ' '),
            '=',
            take_while(0.., |character: char| character == ' '),
            '"',
        )
            .parse_next(input)?;
        let literal = take_while(1.., |character: char| character != '"').parse_next(input)?;
        let _ = '"'.parse_next(input)?;
        if path.last() != Some(&"url") {
            return Err(winnow::error::ContextError::new());
        }
        let name_position = path.len() - 2;
        let name = path[name_position];
        if name == "inputs" {
            return Err(winnow::error::ContextError::new());
        }
        Ok(Self {
            input: InputName::new(name),
            literal: literal.to_string(),
        })
    }
}

/// One input URL literal found in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputUrlOccurrence {
    input: InputName,
    url: InputUrl,
    span: TextSpan,
    edit_state: UrlEditState,
}

/// Whether an occurrence was rewritten this run. Only rewritten literals
/// are substituted on render; untouched literals keep their original bytes
/// (a re-render could drop URL parts outside the modeled grammar, such as
/// query parameters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UrlEditState {
    Untouched,
    Rewritten,
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

impl InputUrl {
    /// Parse one URL literal.
    fn parse(literal: &str) -> Result<Self, String> {
        let Some(body) = literal.strip_prefix("github:") else {
            return Ok(Self::Other(literal.to_string()));
        };
        type GitHubUrlParts<'text> = (&'text str, char, &'text str, Option<(char, &'text str)>);
        let mut input = body;
        let segment = || take_while(1.., |character: char| character != '/' && character != '?');
        let parsed: winnow::Result<GitHubUrlParts<'_>> =
            (segment(), '/', segment(), opt(('/', segment()))).parse_next(&mut input);
        match parsed {
            Ok((owner, _, repository, pin)) => Ok(Self::GitHub {
                owner: owner.to_string(),
                repository: ComponentName::new(repository),
                pin: match pin {
                    Some((_, segment)) => GitHubPin::Pinned(segment.to_string()),
                    None => GitHubPin::Unpinned,
                },
            }),
            Err(_) => Err(format!("github input url unparseable: {literal}")),
        }
    }

    /// Whether the pin segment names an exact revision. Branch- or
    /// tag-shaped segments resolve through the lock and are not rewritten
    /// here.
    pub fn is_revision_pinned(&self) -> bool {
        match self {
            Self::GitHub {
                pin: GitHubPin::Pinned(segment),
                ..
            } => CommitIdentifier::is_full_object_id(segment),
            _ => false,
        }
    }

    /// The owner and repository of a GitHub URL.
    pub fn github_identity(&self) -> Option<(&str, &ComponentName)> {
        match self {
            Self::GitHub {
                owner, repository, ..
            } => Some((owner.as_str(), repository)),
            Self::Other(_) => None,
        }
    }

    /// The URL literal text this value renders to.
    pub fn render(&self) -> String {
        match self {
            Self::GitHub {
                owner,
                repository,
                pin,
            } => match pin {
                GitHubPin::Unpinned => format!("github:{owner}/{}", repository.as_str()),
                GitHubPin::Pinned(segment) => {
                    format!("github:{owner}/{}/{segment}", repository.as_str())
                }
            },
            Self::Other(literal) => literal.clone(),
        }
    }
}

/// The trailing segment of a `github:` input URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubPin {
    /// No third segment: the pin lives in `flake.lock`.
    Unpinned,
    /// A third segment naming a branch, tag, or revision: the pin lives in
    /// the URL and must be rewritten on bump when it is a revision.
    Pinned(String),
}

/// A byte range of the original document text occupied by one URL literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSpan {
    pub start: usize,
    pub end: usize,
}
