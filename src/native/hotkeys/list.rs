//! Hotkey lists are stored as comma-separated strings; a single hotkey is a
//! one-item list. The comma *key* is itself a valid hotkey
//! ("Control+,"): no accelerator ends with "+", so a split segment ending in
//! "+" followed by an empty segment gets its comma back.

const SEPARATOR: char = ',';

/// Splits, trims, de-duplicates and drops empty entries, preserving order.
pub fn parse_hotkey_list(value: &str) -> Vec<String> {
    parse_hotkey_items([value])
}

/// Like [`parse_hotkey_list`] for several stored values (each may hold a list).
pub fn parse_hotkey_items<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let raw: Vec<String> = values
        .into_iter()
        .flat_map(|item| {
            item.as_ref()
                .split(SEPARATOR)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();

    let mut result: Vec<String> = Vec::new();
    for (i, segment) in raw.iter().enumerate() {
        let mut hotkey = segment.trim().to_string();
        if hotkey.ends_with('+') && i + 1 < raw.len() && raw[i + 1].trim().is_empty() {
            hotkey.push(SEPARATOR);
        }
        if hotkey.is_empty() || result.contains(&hotkey) {
            continue;
        }
        result.push(hotkey);
    }
    result
}

/// Canonical comma-separated storage form.
pub fn serialize_hotkey_list(value: &str) -> String {
    parse_hotkey_list(value).join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_single_and_lists() {
        assert_eq!(
            parse_hotkey_list("Control+Shift+R"),
            v(&["Control+Shift+R"])
        );
        assert_eq!(parse_hotkey_list("GLOBE"), v(&["GLOBE"]));
        assert_eq!(
            parse_hotkey_list("GLOBE,Control+Shift+R"),
            v(&["GLOBE", "Control+Shift+R"])
        );
        assert_eq!(
            parse_hotkey_list(" GLOBE , Control+Shift+R "),
            v(&["GLOBE", "Control+Shift+R"])
        );
        assert_eq!(parse_hotkey_list("F8,,F8,F9,"), v(&["F8", "F9"]));
        assert!(parse_hotkey_list("").is_empty());
    }

    #[test]
    fn accepts_arrays_with_nested_lists() {
        assert_eq!(
            parse_hotkey_items(["GLOBE,Control+Alt", " F8 "]),
            v(&["GLOBE", "Control+Alt", "F8"])
        );
        assert_eq!(
            parse_hotkey_items(["Control+,", "F8"]),
            v(&["Control+,", "F8"])
        );
        assert_eq!(
            parse_hotkey_items(["Control++", "F8"]),
            v(&["Control++", "F8"])
        );
    }

    #[test]
    fn serializes_canonically_and_round_trips() {
        assert_eq!(serialize_hotkey_list("F8, F9 ,F8"), "F8,F9");
        assert_eq!(serialize_hotkey_list(""), "");
        let once = serialize_hotkey_list(" GLOBE , Control+Shift+R , MouseButton4 ");
        assert_eq!(once, "GLOBE,Control+Shift+R,MouseButton4");
        assert_eq!(
            parse_hotkey_list(&once),
            v(&["GLOBE", "Control+Shift+R", "MouseButton4"])
        );
    }

    #[test]
    fn keeps_comma_key_hotkeys() {
        assert_eq!(parse_hotkey_list("Control+,"), v(&["Control+,"]));
        assert_eq!(
            parse_hotkey_list("Command+Shift+,"),
            v(&["Command+Shift+,"])
        );
        assert_eq!(
            parse_hotkey_list("GLOBE,Control+,"),
            v(&["GLOBE", "Control+,"])
        );
        assert_eq!(parse_hotkey_list("Control+,,F8"), v(&["Control+,", "F8"]));
        let serialized = parse_hotkey_items(["Control+,", "F8", "GLOBE"]).join(",");
        assert_eq!(serialized, "Control+,,F8,GLOBE");
        assert_eq!(
            parse_hotkey_list(&serialized),
            v(&["Control+,", "F8", "GLOBE"])
        );
    }

    #[test]
    fn preserves_plus_terminated_hotkeys() {
        assert_eq!(parse_hotkey_list("Control++,F8"), v(&["Control++", "F8"]));
        assert_eq!(parse_hotkey_list("Control++,,F8"), v(&["Control++,", "F8"]));
        assert_eq!(
            parse_hotkey_list("Command++,Control+,,"),
            v(&["Command++", "Control+,"])
        );
    }
}
