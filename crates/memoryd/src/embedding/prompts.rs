//! Qwen3-Embedding asymmetric prompt construction.
//!
//! The Qwen3-Embedding family is instruction-aware: query embeddings are
//! computed over a one-line task instruction plus the query, while documents
//! are embedded plain. The model card's recommended format is:
//!
//! ```text
//! Instruct: {task_description}\nQuery: {query}
//! ```
//!
//! with a retrieval task description as the default. Documents receive no
//! prefix. Using the same prompt for both sides — or omitting the instruction
//! on queries — measurably degrades retrieval, so the format is fixed here
//! rather than left to call sites.

/// Memorum-specific query task that biases Qwen3-Embedding toward directly useful memories and lookalike disambiguation.
const DEFAULT_QUERY_TASK: &str = "Given an agent task or user message, retrieve stored memories that are directly useful; distinguish lookalike memories by preferring exact people, projects, identifiers, dates, decisions, constraints, and events over merely similar topics or adjacent names";

/// Wrap a query with the Qwen3 instruction prompt.
pub fn query_prompt(query: &str) -> String {
    format!("Instruct: {DEFAULT_QUERY_TASK}\nQuery: {query}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_prompt_uses_instruct_query_format() {
        let prompt = query_prompt("when did we switch to qwen3");
        assert!(prompt.starts_with("Instruct: "), "{prompt}");
        assert!(prompt.contains("stored memories"), "{prompt}");
        assert!(prompt.contains("distinguish lookalike memories"), "{prompt}");
        assert!(
            prompt.contains("exact people, projects, identifiers, dates, decisions, constraints, and events"),
            "{prompt}"
        );
        assert!(prompt.contains("\nQuery: when did we switch to qwen3"), "{prompt}");
    }
}
