//! What a published dataset says about itself, shared by every dataset this crate publishes.
//!
//! Two questions a file has to answer for a reader who has only the file: where the data came from,
//! and what may be done with it. They are the same two questions whatever the data is, so the
//! answer is one type rather than a block of fields copied into each dataset — copied fields drift,
//! and a dataset that answers the questions differently from its neighbour answers them worse.

/// What a consumer of these datasets may do with them.
///
/// The data is a set of facts — a name and the number that dispatches to it — and facts are weakly
/// protected everywhere and differently in each jurisdiction. Waiving what rights there are removes
/// the question instead of leaving every consumer to answer it per country.
pub const DATASET_LICENCE: &str = "CC0-1.0";

/// What a file written before these fields existed says about itself: nothing.
///
/// Filling either in from today's constants would put this repository's answer into somebody else's
/// file. An unstated field is reported as unstated.
pub const UNSTATED: &str = "unstated";

/// The value an unstated field reads as.
pub(crate) fn unstated_field() -> String {
    UNSTATED.to_string()
}

/// The version a file written before the field existed is read as.
///
/// Pinned rather than following whatever the current constant is: a file with no version field is
/// a file of the shape 1.0 describes, and it stays that shape as the format moves on.
fn schema_version_before_the_field() -> String {
    "1.0".to_string()
}

/// One corpus a dataset was learned from, and where that corpus itself came from.
///
/// Per-entry provenance says which contract a name was seen in; this says where those contracts
/// came from. Both are needed to trace a name back to something a reader can go and look at:
/// `seen_in` names a file, and this names the collection that file belongs to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CorpusSource {
    /// The corpus root, as a path in this repository.
    pub root: String,
    /// What that corpus is, in one line.
    pub what: String,
    /// Where the corpus's own provenance is recorded.
    pub provenance: String,
}

/// The header every dataset this crate publishes carries.
///
/// Flattened into each dataset's own JSON rather than nested under a key: the fields are about the
/// file, not about a part of it, and a reader looking for the licence should find it at the top of
/// the file rather than inside an object they have to know the name of.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DatasetProvenance {
    /// Which version of this file format the dataset is in.
    #[serde(default = "schema_version_before_the_field")]
    pub schema_version: String,
    /// What a consumer may do with the data.
    #[serde(default = "unstated_field")]
    pub licence: String,
    /// The command that rebuilds this file.
    #[serde(default = "unstated_field")]
    pub built_by: String,
    /// Which corpora it was learned from, and where each of those came from.
    #[serde(default)]
    pub sources: Vec<CorpusSource>,
}

impl DatasetProvenance {
    /// A header for a dataset of `schema_version` rebuilt by `built_by`.
    #[must_use]
    pub fn new(schema_version: &str, built_by: &str) -> Self {
        Self {
            schema_version: schema_version.to_string(),
            licence: DATASET_LICENCE.to_string(),
            built_by: built_by.to_string(),
            sources: Vec::new(),
        }
    }

    /// The same header under a licence that is not this repository's to waive.
    ///
    /// A dataset learned from somebody else's sources carries their terms, not ours: waiving rights
    /// in material we did not write would be waiving what is not ours to waive.
    #[must_use]
    pub fn under_licence(mut self, licence: &str) -> Self {
        self.licence = licence.to_string();
        self
    }

    /// Records which corpora the dataset was learned from.
    ///
    /// Sorted by root, for the same reason the data itself is: a build of one corpus produces one
    /// file, and a list in walk order is a diff in whatever order the walk happened to take.
    pub fn record_sources(&mut self, sources: impl IntoIterator<Item = CorpusSource>) {
        self.sources = sources.into_iter().collect();
        self.sources
            .sort_by(|left, right| left.root.cmp(&right.root));
        self.sources.dedup();
    }
}
