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
//! with a generic retrieval task description as the default. Documents receive
//! no prefix. Using the same prompt for both sides — or omitting the
//! instruction on queries — measurably degrades retrieval, so the format is
//! fixed here rather than left to call sites.

/// Default task description from the Qwen3-Embedding model card.
const DEFAULT_QUERY_TASK: &str = "Given a web search query, retrieve relevant passages that answer the query";

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
        assert!(prompt.contains("\nQuery: when did we switch to qwen3"), "{prompt}");
    }
}
