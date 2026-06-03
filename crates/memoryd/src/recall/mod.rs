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
pub(crate) mod source_identity;
pub(crate) mod startup;
pub(crate) mod types;

pub use binding::validate_startup_request;
pub use budget::{estimated_tokens, truncate_utf8_bytes, TruncatedText};
pub use candidates::{
    collect_recall_candidates, collect_recall_candidates_from_index, CandidateCollection, RecallCandidate,
    RecallCollectionRequest, RecallIndexFuture, RecallIndexReader,
};
pub use counters::{RecallStatusCounters, SharedRecallCounters};
pub use delta::{
    build_delta_response, build_delta_response_with_coordination, DeltaCoordinationContext, DeltaPeerCooldownStore,
    DeltaPeerDeliveryRecorder,
};
pub use entity::{resolve_entity_matches, EntityResolution};
pub use error::RecallError;
pub use rank::{
    rank_recall_candidates, select_ranked_candidates, RankedRecallCandidate, RankedSelection, RankingContext,
};
pub use render::{
    escape_xml_attr, escape_xml_text, render_delta_frame, render_memory_entry, render_startup_frame,
    render_startup_frame_with_coordination, render_startup_frame_with_cross_device_updates, CrossDeviceStartupUpdates,
    DeltaRecallItem, RecallEntry, RenderedDeltaFrame, RenderedRecallSection, StartupCoordinationRender,
};
pub use startup::{
    build_startup_response, build_startup_response_with_coordination_config,
    build_startup_response_with_coordination_level,
};
pub use types::{
    bounded_omissions, BoundedOmissions, ConcurrentSessionMode, DeltaPeerDelivery, DeltaRequest, DeltaResponse,
    EntityMatchKind, OmissionReason, ProjectBinding, ProjectBindingSource, RecallExplanation, RecallOmission,
    RecallSectionExplanation, RecallSectionName, SessionBinding, StartupRequest, StartupResponse,
    MAX_SERIALIZED_OMISSIONS, STREAM_E_POLICY,
};
