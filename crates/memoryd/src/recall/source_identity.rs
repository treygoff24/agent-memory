use memory_substrate::{Memory, RecallIndexRow, Substrate};

use crate::recall::types::SessionBinding;

#[derive(Debug)]
pub(crate) struct PeerSourceIdentity {
    pub(crate) harness: String,
    pub(crate) session_id: String,
}

impl PeerSourceIdentity {
    fn from_memory(memory: &Memory) -> Self {
        Self {
            harness: first_present([
                memory.frontmatter.source.harness.as_deref(),
                memory.frontmatter.author.harness.as_deref(),
            ]),
            session_id: first_present([
                memory.frontmatter.source.session_id.as_deref(),
                memory.frontmatter.author.session_id.as_deref(),
            ]),
        }
    }

    fn unknown() -> Self {
        Self { harness: "unknown".to_owned(), session_id: "unknown".to_owned() }
    }

    pub(crate) fn matches_session(&self, session_binding: &SessionBinding) -> bool {
        self.harness == session_binding.harness && self.session_id == session_binding.session_id
    }
}

pub(crate) async fn peer_source_identity(substrate: &Substrate, row: &RecallIndexRow) -> PeerSourceIdentity {
    match substrate.read_memory(&row.id).await {
        Ok(memory) => PeerSourceIdentity::from_memory(&memory),
        Err(_) => PeerSourceIdentity::unknown(),
    }
}

fn first_present<const N: usize>(values: [Option<&str>; N]) -> String {
    values.into_iter().flatten().map(str::trim).find(|value| !value.is_empty()).unwrap_or("unknown").to_owned()
}
