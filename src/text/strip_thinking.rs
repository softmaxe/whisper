//! Strips `<think>…</think>` blocks emitted by reasoning models.
//!
//! Innermost closed pairs go first so nested blocks cannot leave a stray
//! `</think>`, then a trailing unterminated `<think>` is cut, and finally
//! orphan `</think>` tags are dropped. Tags match ASCII case-insensitively.

const OPEN: &[u8] = b"<think>";
const CLOSE: &[u8] = b"</think>";

fn tag_at(lower: &[u8], pos: usize, tag: &[u8]) -> bool {
    lower.get(pos..pos + tag.len()) == Some(tag)
}

/// Byte range of the leftmost `<think>` whose next tag is a `</think>`.
fn innermost_closed(lower: &[u8]) -> Option<(usize, usize)> {
    let mut open: Option<usize> = None;
    for pos in 0..lower.len() {
        if tag_at(lower, pos, OPEN) {
            open = Some(pos);
        } else if tag_at(lower, pos, CLOSE)
            && let Some(start) = open
        {
            return Some((start, pos + CLOSE.len()));
        }
    }
    None
}

pub fn strip_thinking_tags(text: &str) -> String {
    let mut out = text.to_string();
    // Tags are ASCII, so byte offsets in the lowercased copy match `out`.
    while let Some((start, end)) = innermost_closed(out.to_ascii_lowercase().as_bytes()) {
        out.replace_range(start..end, "");
    }
    let lower = out.to_ascii_lowercase();
    if let Some(start) = lower.find("<think>") {
        out.truncate(start);
    }
    loop {
        let lower = out.to_ascii_lowercase();
        let Some(start) = lower.find("</think>") else {
            break;
        };
        out.replace_range(start..start + CLOSE.len(), "");
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_blocks() {
        assert_eq!(
            strip_thinking_tags("<think>let me reason about this</think>Meeting Notes Summary"),
            "Meeting Notes Summary"
        );
        assert_eq!(
            strip_thinking_tags("Title Here<think>reasoning that never closed"),
            "Title Here"
        );
        assert_eq!(
            strip_thinking_tags("Quarterly Plan<think>...</think>"),
            "Quarterly Plan"
        );
        assert_eq!(
            strip_thinking_tags("Project Kickoff Notes"),
            "Project Kickoff Notes"
        );
        assert_eq!(
            strip_thinking_tags("<think>just thinking, no answer</think>"),
            ""
        );
    }

    #[test]
    fn nested_and_sequential() {
        assert_eq!(
            strip_thinking_tags("<think>a<think>b</think>c</think>Answer"),
            "Answer"
        );
        assert_eq!(
            strip_thinking_tags("<think>outer<think>mid<think>inner</think>x</think>y</think>Done"),
            "Done"
        );
        assert_eq!(
            strip_thinking_tags("<think>one</think>Keep<think>two</think>This"),
            "KeepThis"
        );
    }

    #[test]
    fn mixed_case_and_stray_tags() {
        assert_eq!(
            strip_thinking_tags("<Think>reasoning</Think>Meeting Notes"),
            "Meeting Notes"
        );
        assert_eq!(
            strip_thinking_tags("<THINK>reasoning</THINK>Meeting Notes"),
            "Meeting Notes"
        );
        assert_eq!(
            strip_thinking_tags("</think>Meeting Notes"),
            "Meeting Notes"
        );
        assert_eq!(
            strip_thinking_tags("</Think>Meeting Notes"),
            "Meeting Notes"
        );
        assert_eq!(strip_thinking_tags("Title<THINK>unterminated"), "Title");
    }

    #[test]
    fn keeps_non_ascii_text() {
        assert_eq!(
            strip_thinking_tags("<think>思考</think> 会议纪要 "),
            "会议纪要"
        );
    }
}
