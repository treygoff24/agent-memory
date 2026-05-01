pub(crate) mod binding;
pub(crate) mod budget;
pub(crate) mod candidates;
pub(crate) mod counters;
pub(crate) mod delta;
pub(crate) mod dream_questions;
pub(crate) mod entity;
pub(crate) mod error;
pub(crate) mod project;
pub(crate) mod rank;
pub(crate) mod render;
pub(crate) mod startup;
pub(crate) mod types;

pub use binding::validate_startup_request;
pub use budget::{estimated_tokens, truncate_utf8_bytes, TruncatedText};
pub use candidates::{
    collect_recall_candidates, collect_recall_candidates_from_index, CandidateCollection, RecallCandidate,
    RecallCollectionRequest, RecallIndexFuture, RecallIndexReader,
};
pub use counters::{RecallStatusCounters, SharedRecallCounters};
pub use delta::build_delta_response;
pub use entity::{resolve_entity_matches, EntityMatchKind, EntityResolution};
pub use error::RecallError;
pub use rank::{
    rank_recall_candidates, select_ranked_candidates, RankedRecallCandidate, RankedSelection, RankingContext,
};
pub use render::{
    escape_xml_attr, escape_xml_text, render_memory_entry, render_startup_frame, RecallEntry, RenderedRecallSection,
};
pub use startup::build_startup_response;
pub use types::{
    bounded_omissions, BoundedOmissions, DeltaRequest, DeltaResponse, OmissionReason, ProjectBinding,
    ProjectBindingSource, RecallExplanation, RecallOmission, RecallSectionExplanation, RecallSectionName,
    SessionBinding, StartupRequest, StartupResponse, MAX_SERIALIZED_OMISSIONS, STREAM_E_POLICY,
};
