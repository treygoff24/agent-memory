//! Memory tree layout and validation.

mod layout;
mod validate;

pub use layout::{
    bootstrap_repo_layout, bootstrap_repo_tree, has_substrate_marker, memory_dirs, relative_memory_paths,
    DEFAULT_ACTIVE_EMBEDDING_DIMENSION, DEFAULT_ACTIVE_EMBEDDING_MODEL_REF, DEFAULT_ACTIVE_EMBEDDING_PROVIDER,
};
pub use validate::{validate_case_fold_paths, validate_tree, TreeValidationMode, TreeValidationReport};
