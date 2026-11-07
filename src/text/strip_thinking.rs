//! Strips `<think>…</think>` blocks emitted by reasoning models.
//!
//! Nested and unterminated blocks are hidden; orphan closing tags are dropped.
//! Tags match ASCII case-insensitively in a single linear scan.
//! Only tags present in the original input are recognized; removing a block
//! does not cause surrounding text to be reparsed as a new tag.

const OPEN: &[u8] = b"<think>";
const CLOSE: &[u8] = b"</think>";

fn tag_at(bytes: &[u8], pos: usize, tag: &[u8]) -> bool {
    bytes
        .get(pos..pos + tag.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(tag))
}

pub fn strip_thinking_tags(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    let mut pos = 0;
    let mut visible_start = 0;
    while pos < bytes.len() {
        let opening = tag_at(bytes, pos, OPEN);
        let closing = !opening && tag_at(bytes, pos, CLOSE);
        if opening || closing {
            if depth == 0 {
                // ASCII tag boundaries are also UTF-8 boundaries.
                out.push_str(&text[visible_start..pos]);
            }
            if opening {
                depth += 1;
                pos += OPEN.len();
            } else {
                depth = depth.saturating_sub(1);
                pos += CLOSE.len();
            }
            visible_start = pos;
        } else {
            pos += 1;
        }
    }
    if depth == 0 {
        out.push_str(&text[visible_start..]);
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

    #[test]
    fn handles_dense_deep_and_unterminated_blocks() {
        assert_eq!(
            strip_thinking_tags(&format!(
                "{}保留🙂{}",
                "<think>x</think>".repeat(30_000),
                "</THINK>".repeat(30_000)
            )),
            "保留🙂"
        );
        assert_eq!(
            strip_thinking_tags(&format!(
                "前{}秘密{}后",
                "<think>".repeat(30_000),
                "</think>".repeat(30_000)
            )),
            "前后"
        );
        assert_eq!(strip_thinking_tags("前<think>a<think>b</think>秘密"), "前");
        assert_eq!(
            strip_thinking_tags("保留 <thin>🙂</thin>"),
            "保留 <thin>🙂</thin>"
        );
    }

    #[test]
    fn only_recognizes_tags_present_in_the_input() {
        assert_eq!(
            strip_thinking_tags("<thi<think>x</think>nk>answer"),
            "<think>answer"
        );
    }
}
