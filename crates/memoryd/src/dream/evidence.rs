use super::types::{ActiveMemory, EvidenceCatalogEntry, SubstrateFragment};

pub fn build_evidence_catalog(
    substrate_fragments: &[SubstrateFragment],
    active_memories: &[ActiveMemory],
) -> Vec<EvidenceCatalogEntry> {
    let fragment_entries = substrate_fragments.iter().map(|fragment| EvidenceCatalogEntry {
        kind: "substrate_fragment".to_string(),
        reference: fragment.id.clone(),
        entities: fragment.entities.clone(),
        excerpt: fragment.text.clone(),
    });

    let memory_entries = active_memories.iter().map(|memory| EvidenceCatalogEntry {
        kind: "memory".to_string(),
        reference: memory.id.clone(),
        entities: memory.entities.clone(),
        excerpt: memory.summary.clone(),
    });

    fragment_entries.chain(memory_entries).collect()
}
