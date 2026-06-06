use crate::types::{AbilityAST, IgnoredKind, OracleSpan, ability::StaticAbility};

// ── Private helpers ──────────────────────────────────────────────────────────

/// Returns the byte offset of the first `target` char at parenthetical depth 0,
/// or `None` if not found.
fn find_at_depth_zero(text: &str, target: char) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == target && depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Splits `text` on `sep` characters at parenthetical depth 0.
fn split_at_depth_zero<'a>(text: &'a str, sep: char) -> Vec<&'a str> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
                result.push(&text[start..i]);
                start = i + sep.len_utf8();
            }
            _ => {}
        }
    }
    result.push(&text[start..]);
    result
}

/// True if `s` (already lowercased) is one of the eleven evergreen keywords.
fn is_known_keyword(s: &str) -> bool {
    matches!(
        s,
        "flying"
            | "reach"
            | "trample"
            | "first strike"
            | "double strike"
            | "vigilance"
            | "haste"
            | "lifelink"
            | "deathtouch"
            | "menace"
            | "indestructible"
    )
}

/// Emits spans for a single comma-separated token (no top-level em-dash).
/// Extracts any `(…)` reminder text inline, in source order.
fn emit_token_spans(token: &str, spans: &mut Vec<OracleSpan>) {
    // Partition the token into alternating non-paren and paren segments.
    let mut segments: Vec<(bool, &str)> = Vec::new();
    let mut depth = 0usize;
    let mut seg_start = 0usize;

    for (i, c) in token.char_indices() {
        match c {
            '(' if depth == 0 => {
                if i > seg_start {
                    segments.push((false, &token[seg_start..i]));
                }
                seg_start = i;
                depth = 1;
            }
            '(' => depth += 1,
            ')' if depth == 1 => {
                depth = 0;
                let end = i + ')'.len_utf8();
                segments.push((true, &token[seg_start..end]));
                seg_start = end;
            }
            ')' if depth > 0 => depth -= 1,
            _ => {}
        }
    }
    if seg_start < token.len() {
        segments.push((false, &token[seg_start..]));
    }

    // Emit spans in source order; accumulate non-paren text for keyword matching.
    let mut accumulated = String::new();
    for (is_paren, text) in segments {
        if is_paren {
            let kw = accumulated.trim();
            if !kw.is_empty() {
                spans.push(match_keyword(kw));
            }
            accumulated.clear();
            spans.push(OracleSpan::Ignored(
                IgnoredKind::ReminderText,
                text.to_string(),
            ));
        } else {
            accumulated.push_str(text);
        }
    }
    let kw = accumulated.trim();
    if !kw.is_empty() {
        spans.push(match_keyword(kw));
    }
}

fn match_keyword(kw: &str) -> OracleSpan {
    match kw.to_lowercase().as_str() {
        "flying" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Flying)),
        "reach" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Reach)),
        "trample" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Trample)),
        "first strike" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::FirstStrike)),
        "double strike" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::DoubleStrike)),
        "vigilance" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Vigilance)),
        "haste" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Haste)),
        "lifelink" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Lifelink)),
        "deathtouch" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Deathtouch)),
        "menace" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Menace)),
        "indestructible" => OracleSpan::Parsed(AbilityAST::Static(StaticAbility::Indestructible)),
        _ => OracleSpan::Unparsed(kw.to_string()),
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Parse Oracle text into a sequence of typed spans.
///
/// Always succeeds. Separators (`\n`, `,`) are consumed; each logical token
/// becomes one span. See `OracleSpan` for rendering intent.
pub fn parse_oracle_text(text: &str) -> Vec<OracleSpan> {
    const EM_DASH: char = '\u{2014}';
    let mut spans = Vec::new();

    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }

        // Em-dash at depth 0 → ability/flavour word line.
        if let Some(dash_pos) = find_at_depth_zero(paragraph, EM_DASH) {
            let left = paragraph[..dash_pos].trim();
            let right = paragraph[dash_pos + EM_DASH.len_utf8()..].trim();

            if !is_known_keyword(&left.to_lowercase()) {
                // Preserve the raw label text up to and including the em-dash.
                let label = paragraph[..dash_pos + EM_DASH.len_utf8()].to_string();
                spans.push(OracleSpan::Ignored(IgnoredKind::AbilityWord, label));
                if !right.is_empty() {
                    spans.push(OracleSpan::Unparsed(right.to_string()));
                }
                continue;
            }
        }

        // Split on commas at depth 0; classify each token.
        for token in split_at_depth_zero(paragraph, ',') {
            let token = token.trim();
            if !token.is_empty() {
                emit_token_spans(token, &mut spans);
            }
        }
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::StaticAbility;

    fn parsed(kw: StaticAbility) -> OracleSpan {
        OracleSpan::Parsed(AbilityAST::Static(kw))
    }
    fn reminder(text: &str) -> OracleSpan {
        OracleSpan::Ignored(IgnoredKind::ReminderText, text.to_string())
    }
    fn ability_word(text: &str) -> OracleSpan {
        OracleSpan::Ignored(IgnoredKind::AbilityWord, text.to_string())
    }
    fn unparsed(text: &str) -> OracleSpan {
        OracleSpan::Unparsed(text.to_string())
    }

    #[test]
    fn empty_text_returns_empty_vec() {
        assert_eq!(parse_oracle_text(""), vec![]);
    }

    #[test]
    fn blank_lines_skipped() {
        assert_eq!(parse_oracle_text("\n\n"), vec![]);
    }

    #[test]
    fn reminder_text_only() {
        assert_eq!(
            parse_oracle_text("({T}: Add {G}.)"),
            vec![reminder("({T}: Add {G}.)")]
        );
    }

    #[test]
    fn single_keyword() {
        assert_eq!(
            parse_oracle_text("Flying"),
            vec![parsed(StaticAbility::Flying)]
        );
    }

    #[test]
    fn comma_separated_keywords() {
        assert_eq!(
            parse_oracle_text("Flying, vigilance"),
            vec![
                parsed(StaticAbility::Flying),
                parsed(StaticAbility::Vigilance)
            ]
        );
    }

    #[test]
    fn multiline_keywords() {
        assert_eq!(
            parse_oracle_text("Trample\nLifelink"),
            vec![
                parsed(StaticAbility::Trample),
                parsed(StaticAbility::Lifelink)
            ]
        );
    }

    #[test]
    fn two_word_keyword() {
        assert_eq!(
            parse_oracle_text("First strike"),
            vec![parsed(StaticAbility::FirstStrike)]
        );
    }

    #[test]
    fn keyword_with_reminder_text() {
        assert_eq!(
            parse_oracle_text(
                "Deathtouch (Any amount of damage this deals to a creature is enough to destroy it.)"
            ),
            vec![
                parsed(StaticAbility::Deathtouch),
                reminder(
                    "(Any amount of damage this deals to a creature is enough to destroy it.)"
                ),
            ]
        );
    }

    #[test]
    fn ability_word_line_splits_at_em_dash() {
        let result = parse_oracle_text(
            "Landfall \u{2014} Whenever a land you control enters, you gain 1 life.",
        );
        assert_eq!(
            result,
            vec![
                ability_word("Landfall \u{2014}"),
                unparsed("Whenever a land you control enters, you gain 1 life."),
            ]
        );
    }

    #[test]
    fn cumulative_upkeep_style_no_spaces() {
        let result = parse_oracle_text("Cumulative upkeep\u{2014}Add {R}.");
        assert_eq!(
            result,
            vec![
                ability_word("Cumulative upkeep\u{2014}"),
                unparsed("Add {R}."),
            ]
        );
    }

    #[test]
    fn triggered_ability_becomes_unparsed() {
        assert_eq!(
            parse_oracle_text("When this creature enters, draw a card."),
            vec![unparsed("When this creature enters, draw a card.")]
        );
    }

    #[test]
    fn em_dash_inside_parens_not_split() {
        assert_eq!(
            parse_oracle_text("(Choose one \u{2014} do A; or do B.)"),
            vec![reminder("(Choose one \u{2014} do A; or do B.)")]
        );
    }

    #[test]
    fn all_eleven_keywords_parse() {
        let text = "Flying\nReach\nTrample\nFirst strike\nDouble strike\nVigilance\nHaste\nLifelink\nDeathtouch\nMenace\nIndestructible";
        let result = parse_oracle_text(text);
        assert_eq!(result.len(), 11);
        assert!(result.iter().all(|s| matches!(s, OracleSpan::Parsed(_))));
    }

    #[test]
    fn keyword_and_ability_word_on_separate_lines() {
        let text = "Flying\nLandfall \u{2014} Whenever a land you control enters, you gain 1 life.";
        let result = parse_oracle_text(text);
        assert_eq!(
            result,
            vec![
                parsed(StaticAbility::Flying),
                ability_word("Landfall \u{2014}"),
                unparsed("Whenever a land you control enters, you gain 1 life."),
            ]
        );
    }
}
