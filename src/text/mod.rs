//! Pure, deterministic text utilities.

pub mod api_error;
pub mod chinese_script;
pub mod correction_learner;
pub mod dictionary_import;
pub mod echo_filter;
pub mod format;
pub mod hotkeys;
pub mod language;
pub mod prompt_cap;
pub mod prompts;
mod script;
pub mod snippets;
pub mod strip_thinking;
pub mod url;
mod word_segment;

/// The agent name substituted into `{{agentName}}` in cleanup prompts: the
/// app's own name.
pub const DEFAULT_AGENT_NAME: &str = "Whisper";
