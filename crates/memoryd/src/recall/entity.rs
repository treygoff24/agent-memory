use std::collections::{BTreeMap, BTreeSet};

use crate::recall::candidates::RecallCandidate;
use crate::recall::types::{OmissionReason, RecallOmission, RecallSectionName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityMatchKind {
    None,
    Tag,
    ExactLabelOrAlias,
    ExactId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntityResolution {
    pub candidates: Vec<RecallCandidate>,
    pub omitted: Vec<RecallOmission>,
}

pub fn resolve_entity_matches(
    section: RecallSectionName,
    candidates: Vec<RecallCandidate>,
    seeds: &[&str],
) -> EntityResolution {
    let seeds = seeds.iter().map(|seed| Seed::new(seed)).collect::<Vec<_>>();
    if seeds.is_empty() {
        return EntityResolution { candidates, omitted: Vec::new() };
    }

    let collisions = alias_collisions(&candidates, &seeds);
    let ambiguous_aliases = collisions.keys().cloned().collect::<BTreeSet<_>>();
    let candidates =
        candidates.into_iter().filter_map(|candidate| match_candidate(candidate, &seeds, &ambiguous_aliases)).collect();
    EntityResolution { candidates, omitted: collision_omissions(section, collisions) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Seed {
    raw: String,
    normalized: Option<String>,
}

impl Seed {
    fn new(value: &str) -> Self {
        Self { raw: value.trim().to_owned(), normalized: normalized_match_term(value) }
    }
}

fn match_candidate(
    mut candidate: RecallCandidate,
    seeds: &[Seed],
    ambiguous_aliases: &BTreeSet<String>,
) -> Option<RecallCandidate> {
    let mut best = EntityMatchKind::None;
    for seed in seeds {
        best = best.max(match_kind(&candidate, seed, ambiguous_aliases));
    }

    (best != EntityMatchKind::None).then(|| {
        candidate.entity_match = best;
        candidate
    })
}

fn match_kind(candidate: &RecallCandidate, seed: &Seed, ambiguous_aliases: &BTreeSet<String>) -> EntityMatchKind {
    if candidate.row.entities.iter().any(|entity| entity.id == seed.raw) {
        return EntityMatchKind::ExactId;
    }
    let Some(normalized_seed) = seed.normalized.as_deref() else {
        return EntityMatchKind::None;
    };
    if candidate
        .row
        .entities
        .iter()
        .any(|entity| normalized_match_term(&entity.label).as_deref() == Some(normalized_seed))
        || candidate
            .row
            .entities
            .iter()
            .flat_map(|entity| entity.aliases.iter())
            .filter_map(|alias| normalized_match_term(alias))
            .any(|alias| alias == normalized_seed && !ambiguous_aliases.contains(&alias))
        || candidate.row.aliases.iter().any(|alias| normalized_match_term(alias).as_deref() == Some(normalized_seed))
    {
        return EntityMatchKind::ExactLabelOrAlias;
    }
    if candidate.row.tags.iter().any(|tag| normalized_match_term(tag).as_deref() == Some(normalized_seed)) {
        return EntityMatchKind::Tag;
    }
    EntityMatchKind::None
}

fn alias_collisions(candidates: &[RecallCandidate], seeds: &[Seed]) -> BTreeMap<String, Vec<String>> {
    let seed_set = seeds.iter().filter_map(|seed| seed.normalized.as_ref()).collect::<BTreeSet<_>>();
    let mut alias_to_entity_ids: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for candidate in candidates {
        for entity in &candidate.row.entities {
            for alias in &entity.aliases {
                let Some(normalized_alias) = normalized_match_term(alias) else {
                    continue;
                };
                if seed_set.contains(&normalized_alias) {
                    alias_to_entity_ids.entry(normalized_alias).or_default().insert(entity.id.clone());
                }
            }
        }
    }

    alias_to_entity_ids
        .into_iter()
        .filter_map(|(alias, ids)| (ids.len() > 1).then(|| (alias, ids.into_iter().collect())))
        .collect()
}

fn collision_omissions(section: RecallSectionName, collisions: BTreeMap<String, Vec<String>>) -> Vec<RecallOmission> {
    collisions
        .into_iter()
        .map(|(alias, colliding_ids)| RecallOmission {
            id: None,
            section,
            reason: OmissionReason::AmbiguousAlias,
            alias: Some(alias),
            colliding_ids,
        })
        .collect()
}

fn normalized_match_term(value: &str) -> Option<String> {
    let mut normalized = String::new();
    let mut last_was_separator = false;
    let mut alphanumeric_count = 0;

    for character in value.trim().chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
            last_was_separator = false;
            alphanumeric_count += 1;
        } else if (matches!(character, '-' | '_' | '/' | ' ') || character.is_ascii_whitespace())
            && !normalized.is_empty()
            && !last_was_separator
        {
            normalized.push(' ');
            last_was_separator = true;
        }
    }

    let normalized = normalized.trim().to_owned();
    if normalized.is_empty() || alphanumeric_count < 3 {
        return None;
    }
    Some(normalized)
}

impl EntityMatchKind {
    pub fn weight(self) -> i64 {
        match self {
            Self::None => 0,
            Self::Tag => 10,
            Self::ExactLabelOrAlias => 25,
            Self::ExactId => 40,
        }
    }
}

impl Ord for EntityMatchKind {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.weight().cmp(&other.weight())
    }
}

impl PartialOrd for EntityMatchKind {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
