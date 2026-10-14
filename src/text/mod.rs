//! Pure, deterministic text utilities.

pub mod agent_name;
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

pub use agent_name::DEFAULT_AGENT_NAME;
pub use api_error::extract_api_error_message;
pub use chinese_script::{
    ChineseScriptTarget, apply_chinese_script, is_chinese_text, merge_whisper_prompt,
    resolve_chinese_script_target, resolve_cleanup_language,
};
pub use correction_learner::extract_corrections;
pub use dictionary_import::{format_dictionary_export, parse_dictionary_import_text};
pub use echo_filter::{
    DICTIONARY_ECHO_CODE, analyze_dictionary_prompt_fragment, matches_dictionary_prompt,
};
pub use format::{DateGroup, date_group, format_bytes, format_mm_ss, normalize_db_date};
pub use hotkeys::{
    HotkeyValidationError, format_hotkey_display, format_hotkey_label, format_hotkey_list_label,
    parse_hotkey_list, serialize_hotkey_list, validate_hotkey,
};
pub use language::{Language, get_language_instruction, languages};
pub use prompt_cap::{WHISPER_DECODER_PROMPT_CHARS, WHISPER_PROMPT_CHARS, fit_dictionary_prompt};
pub use prompts::{
    ChatParamsConfig, apply_chat_completions_params, resolve_cleanup_prompt,
    resolve_prompt_template, wrap_cleanup_transcript,
};
pub use snippets::{
    MAX_SNIPPET_TRIGGER_LENGTH, Snippet, SnippetMatcher, expand_snippets,
    get_dictionary_hint_words, get_prioritized_dictionary_hint_words,
};
pub use strip_thinking::strip_thinking_tags;
pub use url::{build_api_url, is_secure_http_endpoint, normalize_base_url};
