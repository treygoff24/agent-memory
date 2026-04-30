pub mod binding;
pub mod budget;
pub mod candidates;
pub mod counters;
pub mod delta;
pub mod entity;
pub mod error;
pub mod project;
pub mod rank;
pub mod render;
pub mod startup;
pub mod types;

pub use binding::validate_startup_request;
pub use budget::{estimated_tokens, truncate_utf8_bytes, TruncatedText};
pub use candidates::{
    collect_recall_candidates, collect_recall_candidates_from_index, CandidateCollection, RecallCandidate,
    RecallCollectionRequest, RecallIndexFuture, RecallIndexReader,
};
pub use counters::{RecallCounters, SharedRecallCounters};
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
