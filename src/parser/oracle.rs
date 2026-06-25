use crate::types::RulesText::ParsedUnimplemented;
use crate::types::ability::{AnnotationKind, Cost, CostComponent, TextAnnotation};
use crate::types::effect::{Effect, EffectStep};
use crate::types::mana::{ManaColor, ManaCost, ManaPip, ManaPool};
use crate::types::{
    IgnoredKind, LandwalkKind, Rule, RulesText,
    ability::{ActivatedAbility, KeywordAbility},
};

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
fn split_at_depth_zero(text: &str, sep: char) -> Vec<&str> {
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

/// Removes all `(...)` reminder text (CR 207.2 — has no effect on gameplay).
fn strip_reminder_text(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Returns the byte offset of the first `:` at depth 0,
/// tracking both `{`/`}` and `(`/`)` as nesting delimiters.
fn find_colon_at_depth_zero(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' | '{' => depth += 1,
            ')' | '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Maps a single-char color code ("W", "U", "B", "R", "G", "C") to a `ManaColor`.
fn oracle_color_from_str(s: &str) -> Option<ManaColor> {
    match s {
        "W" => Some(ManaColor::White),
        "U" => Some(ManaColor::Blue),
        "B" => Some(ManaColor::Black),
        "R" => Some(ManaColor::Red),
        "G" => Some(ManaColor::Green),
        "C" => Some(ManaColor::Colorless),
        _ => None,
    }
}

/// CR 107.4: parse every mana symbol in an activation cost.
/// Returns None for unknown tokens (falls back to ParsedUnimplemented).
fn try_parse_mana_cost(s: &str) -> Option<ManaCost> {
    let mut pips = Vec::new();
    let mut chars = s.chars().peekable();
    let mut saw = false;
    while let Some(c) = chars.next() {
        if c != '{' {
            return None;
        }
        let mut token = String::new();
        for inner in chars.by_ref() {
            if inner == '}' {
                break;
            }
            token.push(inner);
        }
        let parts: Vec<&str> = token.split('/').collect();
        let pip: Option<ManaPip> = match parts.as_slice() {
            ["W"] => Some(ManaPip::White),
            ["U"] => Some(ManaPip::Blue),
            ["B"] => Some(ManaPip::Black),
            ["R"] => Some(ManaPip::Red),
            ["G"] => Some(ManaPip::Green),
            ["C"] => Some(ManaPip::Colorless),
            ["X"] => Some(ManaPip::X),
            ["S"] => Some(ManaPip::Snow),
            [n] => n.parse::<u32>().ok().map(ManaPip::Generic),
            [a, "P"] => oracle_color_from_str(a).map(ManaPip::Phyrexian),
            [a, b] => {
                let ca = oracle_color_from_str(a);
                let cb = oracle_color_from_str(b);
                match (ca, cb) {
                    (Some(_), Some(c2)) if *a == "C" => Some(ManaPip::ColorlessHybrid(c2)),
                    (Some(c1), Some(c2)) => Some(ManaPip::Hybrid(c1, c2)),
                    (None, Some(c2)) => {
                        a.parse::<u32>().ok().map(|n| ManaPip::GenericHybrid(n, c2))
                    }
                    _ => None,
                }
            }
            [a, b, "P"] => match (oracle_color_from_str(a), oracle_color_from_str(b)) {
                (Some(c1), Some(c2)) => Some(ManaPip::HybridPhyrexian(c1, c2)),
                _ => None,
            },
            _ => None,
        };
        pip?; // unknown token → return None (activation cost not recognized)
        pips.push(pip.unwrap());
        saw = true;
    }
    if saw { Some(ManaCost { pips }) } else { None }
}

/// Parses a sequence of colored mana symbols (e.g. `{W}{U}`) into a `ManaPool`.
/// Generic mana (`{2}`) is not supported here — returns `None` for unknown tokens.
/// Used for `AddMana` effect steps (activated mana abilities, e.g. `{T}: Add {G}{G}`).
fn try_parse_mana_pool(s: &str) -> Option<ManaPool> {
    let mut pool = ManaPool::default();
    let mut chars = s.chars().peekable();
    let mut saw_symbol = false;
    while let Some(c) = chars.next() {
        if c != '{' {
            return None;
        }
        let mut token = String::new();
        for inner in chars.by_ref() {
            if inner == '}' {
                break;
            }
            token.push(inner);
        }
        match token.as_str() {
            "W" => pool.white += 1,
            "U" => pool.blue += 1,
            "B" => pool.black += 1,
            "R" => pool.red += 1,
            "G" => pool.green += 1,
            "C" => pool.colorless += 1,
            _ => return None, // no generic mana in add effects
        }
        saw_symbol = true;
    }
    if saw_symbol { Some(pool) } else { None }
}

/// Parses "one"–"ten" or a numeric literal to a `u32`. Used wherever oracle text uses
/// English number words (e.g. "draw three cards", "mill two").
fn parse_number_word(s: &str) -> Option<u32> {
    match s {
        "one" | "1" => Some(1),
        "two" | "2" => Some(2),
        "three" | "3" => Some(3),
        "four" | "4" => Some(4),
        "five" | "5" => Some(5),
        "six" | "6" => Some(6),
        "seven" | "7" => Some(7),
        "eight" | "8" => Some(8),
        "nine" | "9" => Some(9),
        "ten" | "10" => Some(10),
        _ => s.parse().ok(),
    }
}

/// Parses an activated ability cost string (the text before `:`) into a `Cost` vec.
/// Handles `{T}` → `Tap`, mana costs `{W}{2}` etc. → `Mana`, and anything else → `Unimplemented`.
fn parse_activation_cost(s: &str) -> Cost {
    s.split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|token| {
            if token == "{T}" {
                CostComponent::Tap
            } else if let Some(cost) = try_parse_mana_cost(token) {
                CostComponent::Mana(cost)
            } else {
                CostComponent::Unimplemented(token.to_string())
            }
        })
        .collect()
}

/// Attempts to parse a single effect clause string into an `EffectStep`.
/// Returns `None` for patterns that aren't yet recognised.
fn try_parse_effect_step(s: &str) -> Option<EffectStep> {
    let lower = s.to_lowercase();
    let lower = lower.as_str();
    if lower.starts_with("add ") {
        let mana_str = s["add ".len()..].trim();
        return try_parse_mana_pool(mana_str).map(EffectStep::AddMana);
    }
    if lower == "draw a card" {
        return Some(EffectStep::DrawCard(1));
    }
    if lower.starts_with("draw ") && lower.ends_with(" cards") {
        let middle = &lower["draw ".len()..lower.len() - " cards".len()];
        if let Some(n) = parse_number_word(middle) {
            return Some(EffectStep::DrawCard(n));
        }
    }
    if let Some(stripped) = lower.strip_prefix("mill ") {
        let rest = stripped.trim_end_matches(" cards");
        if let Some(n) = parse_number_word(rest.trim()) {
            return Some(EffectStep::Mill(n));
        }
    }
    let stripped = lower
        .strip_prefix("you gain ")
        .or_else(|| lower.strip_prefix("gain "));
    if let Some(rest) = stripped {
        let s = rest.trim_end_matches(" life").trim();
        if let Some(n) = parse_number_word(s) {
            return Some(EffectStep::GainLife(n));
        }
    }
    // "gets +N/+M until end of turn"
    if let Some(rest) = lower.strip_prefix("gets ")
        && let Some(boost_str) = rest.strip_suffix(" until end of turn")
    {
        let boost_str = boost_str.trim();
        if boost_str.starts_with('+') || boost_str.starts_with('-') {
            let parts: Vec<&str> = boost_str.splitn(2, '/').collect();
            if parts.len() == 2 {
                let power_s = parts[0].trim_start_matches('+');
                let toughness_s = parts[1].trim_start_matches('+');
                if let (Ok(p), Ok(t)) = (power_s.parse::<i32>(), toughness_s.parse::<i32>()) {
                    use crate::types::PTDelta;
                    return Some(EffectStep::BoostPermanentPT(PTDelta {
                        power: p,
                        toughness: t,
                    }));
                }
            }
        }
    }
    // "deals N damage"
    if let Some(rest) = lower.strip_prefix("deals ")
        && let Some(damage_str) = rest.strip_suffix(" damage")
        && let Ok(n) = damage_str.trim().parse::<u32>()
    {
        return Some(EffectStep::DealDamage(crate::types::effect::DamageStep {
            amount: n,
            ..Default::default()
        }));
    }
    None
}

/// Parses the effect portion of an activated ability (after the `:`) into an `Effect`.
/// Splits on ". " sentence boundaries; returns `None` if any step fails to parse.
fn parse_ability_effect(s: &str) -> Option<Effect> {
    let s = s.trim_end_matches('.');
    s.split(". ")
        .filter(|step| !step.is_empty())
        .map(|step| try_parse_effect_step(step.trim()))
        .collect()
}

/// Leniently parses a single oracle-text paragraph as a list of effect steps.
/// Splits on ". " (sentence boundary) and ", then " (intra-sentence linking).
/// Steps that cannot be parsed become EffectStep::Unimplemented.
fn parse_spell_effect(paragraph: &str) -> Effect {
    let paragraph = paragraph.trim_end_matches('.');
    paragraph
        .split(". ")
        .flat_map(|sentence| {
            let sentence = sentence.trim_start_matches("Then ").trim();
            sentence.split(", then ").map(|step| {
                let step = step.trim();
                try_parse_effect_step(step)
                    .unwrap_or_else(|| EffectStep::Unimplemented(step.to_string()))
            })
        })
        .collect()
}

/// Returns the byte offset of `sub` within `whole`.
/// Panics in debug builds if `sub` is not a subslice of `whole`.
fn subslice_offset(whole: &str, sub: &str) -> usize {
    debug_assert!(
        sub.as_ptr() as usize >= whole.as_ptr() as usize
            && sub.as_ptr() as usize + sub.len() <= whole.as_ptr() as usize + whole.len(),
        "sub is not a subslice of whole"
    );
    sub.as_ptr() as usize - whole.as_ptr() as usize
}

/// Pushes a `TextAnnotation` for a keyword span if the span kind warrants one.
/// `raw_start..raw_end` is the byte range of the *untrimmed* non-paren text in `original`.
fn push_keyword_annotation(
    span: &RulesText,
    raw_start: usize,
    raw_end: usize,
    original: &str,
    annotations: &mut Vec<TextAnnotation>,
) {
    let kind = match span {
        RulesText::Unparsed(_) => AnnotationKind::Unparsed,
        RulesText::ParsedUnimplemented(_) => AnnotationKind::ParsedUnimplemented,
        RulesText::Active(_) => AnnotationKind::Active,
        _ => return,
    };
    let raw_slice = &original[raw_start..raw_end];
    let trimmed = raw_slice.trim(); // str::trim returns a subslice; safe for subslice_offset
    if trimmed.is_empty() {
        return;
    }
    let trim_start = subslice_offset(original, trimmed);
    annotations.push(TextAnnotation {
        start: trim_start,
        end: trim_start + trimmed.len(),
        kind,
    });
}

/// Emits spans for a single comma-separated token (no top-level em-dash).
/// Extracts any `(…)` reminder text inline, in source order.
/// Also emits `TextAnnotation` values for reminder text and non-parsed keyword spans.
fn emit_token_spans(
    token: &str,
    original: &str,
    spans: &mut Vec<RulesText>,
    annotations: &mut Vec<TextAnnotation>,
) {
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

    // Track byte range of the current non-paren accumulation in `original`.
    let mut acc_start: Option<usize> = None;
    let mut acc_end: usize = 0;
    let mut accumulated = String::new();

    for (is_paren, text) in &segments {
        if *is_paren {
            // Flush accumulated keyword.
            let kw = accumulated.trim();
            if !kw.is_empty() {
                let span = match_keyword(kw);
                push_keyword_annotation(&span, acc_start.unwrap(), acc_end, original, annotations);
                spans.push(span);
            }
            accumulated.clear();
            acc_start = None;

            // Emit reminder text annotation and span.
            let off = subslice_offset(original, text);
            annotations.push(TextAnnotation {
                start: off,
                end: off + text.len(),
                kind: AnnotationKind::ReminderText,
            });
            spans.push(RulesText::Ignored(
                IgnoredKind::ReminderText,
                text.to_string(),
            ));
        } else {
            let off = subslice_offset(original, text);
            if acc_start.is_none() {
                acc_start = Some(off);
            }
            acc_end = off + text.len();
            accumulated.push_str(text);
        }
    }

    // Flush remaining keyword.
    let kw = accumulated.trim();
    if !kw.is_empty() {
        let span = match_keyword(kw);
        push_keyword_annotation(&span, acc_start.unwrap(), acc_end, original, annotations);
        spans.push(span);
    }
}

/// Maps a lowercased quality string (after stripping "protection from " or "hexproof from ")
/// to a `ProtectionQuality`. Returns `None` for unrecognised qualities.
fn parse_protection_quality(s: &str) -> Option<crate::types::ability::ProtectionQuality> {
    use crate::types::ability::ProtectionQuality;
    use crate::types::card::CardType;
    match s {
        "white" => Some(ProtectionQuality::Color(ManaColor::White)),
        "blue" => Some(ProtectionQuality::Color(ManaColor::Blue)),
        "black" => Some(ProtectionQuality::Color(ManaColor::Black)),
        "red" => Some(ProtectionQuality::Color(ManaColor::Red)),
        "green" => Some(ProtectionQuality::Color(ManaColor::Green)),
        "everything" => Some(ProtectionQuality::Everything),
        "artifacts" | "artifact" => Some(ProtectionQuality::CardType(CardType::Artifact)),
        "creatures" | "creature" => Some(ProtectionQuality::CardType(CardType::Creature)),
        "instants" | "instant" => Some(ProtectionQuality::CardType(CardType::Instant)),
        "enchantments" | "enchantment" => Some(ProtectionQuality::CardType(CardType::Enchantment)),
        "sorceries" | "sorcery" => Some(ProtectionQuality::CardType(CardType::Sorcery)),
        "lands" | "land" => Some(ProtectionQuality::CardType(CardType::Land)),
        other => {
            // "[subtype] creatures" / "[subtype] creature" → CreatureType
            let subtype = other
                .strip_suffix(" creatures")
                .or_else(|| other.strip_suffix(" creature"));
            if let Some(sub) = subtype
                && !sub.is_empty()
            {
                let mut chars = sub.chars();
                let title = chars
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>())
                    .unwrap_or_default()
                    + chars.as_str();
                return Some(ProtectionQuality::CreatureType(title));
            }
            None
        }
    }
}

/// Maps a keyword string (already lowercased) to its `RulesText` representation.
/// Returns `Active(Rule::Static(...))` for implemented keywords, `ParsedUnimplemented`
/// for known CR 702 keywords that aren't yet enforced, and `Unparsed` for anything else.
fn match_keyword(kw: &str) -> RulesText {
    let s = kw.to_lowercase();
    let s = s.as_str();

    // ── Fully-implemented keywords ────────────────────────────────────────────
    macro_rules! active {
        ($variant:ident) => {
            RulesText::Active(Rule::Static(KeywordAbility::$variant))
        };
    }
    match s {
        "flying" => return active!(Flying),
        "reach" => return active!(Reach),
        "trample" => return active!(Trample),
        "first strike" => return active!(FirstStrike),
        "double strike" => return active!(DoubleStrike),
        "vigilance" => return active!(Vigilance),
        "haste" => return active!(Haste),
        "lifelink" => return active!(Lifelink),
        "deathtouch" => return active!(Deathtouch),
        "menace" => return active!(Menace),
        "indestructible" => return active!(Indestructible),
        "defender" => return active!(Defender),
        "shadow" => return active!(Shadow),
        "horsemanship" => return active!(Horsemanship),
        "skulk" => return active!(Skulk),
        "decayed" => return active!(Decayed),
        "flash" => return active!(Flash),
        "exalted" => return active!(Exalted),
        "flanking" => return active!(Flanking),
        "melee" => return active!(Melee),
        "prowess" => return active!(Prowess),
        "shroud" => return active!(Shroud),
        "hexproof" => return active!(Hexproof),
        // CR 702.80 Wither
        "wither" => return active!(Wither),
        // CR 702.90 Infect
        "infect" => return active!(Infect),
        // CR 702.100 Evolve
        "evolve" => return active!(Evolve),
        // CR 702.149 Training
        "training" => return active!(Training),
        _ => {}
    }

    // BushidoN: "bushido N"
    if let Some(rest) = s.strip_prefix("bushido ")
        && let Some(n) = parse_number_word(rest.trim())
    {
        return RulesText::Active(Rule::Static(KeywordAbility::BushidoN(n)));
    }

    // CR 702.164 Toxic N
    if let Some(rest) = s.strip_prefix("toxic ")
        && let Ok(n) = rest.trim().parse::<u32>()
    {
        return RulesText::Active(Rule::Static(KeywordAbility::ToxicN(n)));
    }

    // Plain cycling (not type-cycling variants like mountaincycling).
    // Use original `kw` for the cost slice so mana symbols stay uppercase ({U} not {u}).
    if s.starts_with("cycling ")
        && let Some(cost) = try_parse_mana_cost(kw["cycling ".len()..].trim())
    {
        return RulesText::Active(Rule::Cycling(cost));
    }

    // Kicker [cost] (702.33a): optional additional mana cost.
    if s.starts_with("kicker ")
        && let Some(cost) = try_parse_mana_cost(kw["kicker ".len()..].trim())
    {
        return RulesText::Active(Rule::Kicker {
            additional_cost: cost,
        });
    }

    // Multikicker [cost] (702.33c): repeatable additional mana cost.
    if s.starts_with("multikicker ")
        && let Some(cost) = try_parse_mana_cost(kw["multikicker ".len()..].trim())
    {
        return RulesText::Active(Rule::Multikicker {
            additional_cost: cost,
        });
    }

    // Dash [cost] (702.109a): alternative cost; grants Haste; returns to hand at end step.
    if s.starts_with("dash ")
        && let Some(cost) = try_parse_mana_cost(kw["dash ".len()..].trim())
    {
        return RulesText::Active(Rule::Dash {
            alternative_cost: cost,
        });
    }

    // Evoke [cost] (702.74a): alternative cost; ETB trigger sacrifices the permanent.
    if s.starts_with("evoke ")
        && let Some(cost) = try_parse_mana_cost(kw["evoke ".len()..].trim())
    {
        return RulesText::Active(Rule::Evoke {
            alternative_cost: cost,
        });
    }

    // Fear (CR 702.36)
    if s == "fear" {
        return RulesText::Active(Rule::Static(KeywordAbility::Fear));
    }

    // Intimidate (CR 702.13)
    if s == "intimidate" {
        return RulesText::Active(Rule::Static(KeywordAbility::Intimidate));
    }

    // Battle Cry (CR 702.91)
    if s == "battle cry" {
        return RulesText::Active(Rule::Static(KeywordAbility::BattleCry));
    }

    // Ward {cost} (CR 702.21a) — mana cost form e.g. "Ward {2}"
    // Ward is a triggered ability (CR 702.21a): emitted as TriggeredAbility { trigger: TargetedBy }.
    if let Some(_rest) = s.strip_prefix("ward ")
        && let Some(cost) = try_parse_mana_cost(kw["ward ".len()..].trim())
    {
        use crate::types::ability::{
            CostComponent, TriggerEvent, TriggerTargetMode, TriggeredAbility, TurnOwner,
        };
        let components = vec![CostComponent::Mana(cost)];
        return RulesText::Active(Rule::Triggered(TriggeredAbility {
            trigger: TriggerEvent::TargetedBy {
                controller: TurnOwner::Opponent,
            },
            condition: None,
            target_mode: TriggerTargetMode::None,
            effect: vec![EffectStep::Payment {
                cost: components,
                on_paid: vec![],
                on_declined: vec![EffectStep::CounterSpell],
            }],
        }));
    }

    // Protection from [quality] (CR 702.16)
    if let Some(q_str) = s.strip_prefix("protection from ") {
        let quality_str = q_str.trim_end_matches('.');
        if let Some(q) = parse_protection_quality(quality_str) {
            return RulesText::Active(Rule::Static(KeywordAbility::ProtectionFrom(q)));
        }
        return ParsedUnimplemented(kw.to_string());
    }

    // Hexproof from [quality] (CR 702.11d)
    if let Some(q_str) = s.strip_prefix("hexproof from ") {
        let quality_str = q_str.trim_end_matches('.');
        if let Some(q) = parse_protection_quality(quality_str) {
            return RulesText::Active(Rule::Static(KeywordAbility::HexproofFrom(q)));
        }
        return ParsedUnimplemented(kw.to_string());
    }

    // Landwalk (CR 702.14): ends with "walk", prefix identifies the land type.
    // "nonbasic landwalk" and "non-basic landwalk" map to LandwalkKind::Nonbasic;
    // all others map to LandwalkKind::LandType with a title-cased land type name.
    if let Some(prefix) = s.strip_suffix("walk") {
        let kind = if prefix == "nonbasic land" || prefix == "non-basic land" {
            LandwalkKind::Nonbasic
        } else {
            let type_name = match prefix.trim_end() {
                "island" => "Island",
                "swamp" => "Swamp",
                "forest" => "Forest",
                "mountain" => "Mountain",
                "plains" => "Plains",
                other => {
                    let mut chars = other.chars();
                    return RulesText::Active(Rule::Static(KeywordAbility::Landwalk(
                        LandwalkKind::LandType(
                            chars
                                .next()
                                .map(|ch| ch.to_uppercase().collect::<String>())
                                .unwrap_or_default()
                                + chars.as_str(),
                        ),
                    )));
                }
            };
            LandwalkKind::LandType(type_name.to_string())
        };
        return RulesText::Active(Rule::Static(KeywordAbility::Landwalk(kind)));
    }

    // ── CR 702 recognised-but-unimplemented keywords ──────────────────────────
    if is_cr702_keyword(s) {
        return ParsedUnimplemented(kw.to_string());
    }

    RulesText::Unparsed(kw.to_string())
}

/// Returns true if `s` (lowercased) matches any CR 702 keyword pattern.
/// Does NOT include the 14 implemented keywords (those are handled above).
fn is_cr702_keyword(s: &str) -> bool {
    // The keyword part of a token is everything before the first '{' (cost).
    // Used for cycling/landwalk/offering pattern checks.
    let kw_part = s.split('{').next().unwrap_or(s).trim_end();

    matches!(
        s,
        // 702.3 Defender is implemented; listed here for documentation completeness.
        // 702.11 hexproof — implemented
        // 702.13 intimidate — promoted to KeywordAbility::Intimidate
        // 702.18 shroud — implemented
        // 702.22
        "banding" |
        // 702.25
        "flanking" |
        // 702.26
        "phasing" |
        // 702.36 fear — promoted to KeywordAbility::Fear
        // 702.39
        "provoke" |
        // 702.40
        "storm" |
        // 702.44
        "sunburst" |
        // 702.50
        "epic" |
        // 702.51
        "convoke" |
        // 702.55
        "haunt" |
        // 702.61
        "split second" |
        // 702.66
        "delve" |
        // 702.69
        "gravestorm" |
        // 702.73
        "changeling" |
        // 702.78
        "conspire" |
        // 702.79
        "persist" |
        // 702.80 wither — promoted to KeywordAbility::Wither
        // 702.81
        "retrace" |
        // 702.83
        "exalted" |
        // 702.85
        "cascade" |
        // 702.88
        "rebound" |
        // 702.89
        "umbra armor" |
        // 702.90 infect — promoted to KeywordAbility::Infect
        // 702.91 battle cry — promoted to KeywordAbility::BattleCry
        // 702.92
        "living weapon" |
        // 702.93
        "undying" |
        // 702.95
        "soulbond" |
        // 702.98
        "unleash" |
        // 702.99
        "cipher" |
        // 702.100 evolve — promoted to KeywordAbility::Evolve
        // 702.101
        "extort" |
        // 702.102
        "fuse" |
        // 702.105
        "dethrone" |
        // 702.106
        "hidden agenda" |
        // 702.108
        "prowess" |
        // 702.110
        "exploit" |
        // 702.114
        "devoid" |
        // 702.115
        "ingest" |
        // 702.116
        "myriad" |
        // 702.118 skulk — implemented
        // "skulk" |
        // 702.121
        "melee" |
        // 702.124
        "partner" |
        // 702.125
        "undaunted" |
        // 702.126
        "improvise" |
        // 702.127
        "aftermath" |
        // 702.131
        "ascend" |
        // 702.132
        "assist" |
        // 702.133
        "jump-start" |
        // 702.134
        "mentor" |
        // 702.136
        "riot" |
        // 702.139
        "companion" |
        // 702.144
        "demonstrate" |
        // 702.145
        "daybound" | "nightbound" |
        // 702.147 decayed — implemented
        // "decayed" |
        // 702.149 training — promoted to KeywordAbility::Training
        // 702.150
        "compleated" |
        // 702.154
        "enlist" |
        // 702.155
        "read ahead" |
        // 702.156
        "ravenous" |
        // 702.158
        "space sculptor" |
        // 702.159
        "visit" |
        // 702.161
        "living metal" |
        // 702.163
        "for mirrodin!" |
        // 702.166
        "bargain" |
        // 702.169
        "solved" |
        // 702.172
        "spree" |
        // 702.177
        "exhaust" |
        // 702.178
        "max speed" |
        // 702.179
        "start your engines!" |
        // 702.180
        "harmonize" |
        // 702.181
        "mobilize" |
        // 702.182
        "job select" |
        // 702.183
        "tiered" |
        // 702.184
        "station" |
        // 702.187
        "mayhem" |
        // 702.188
        "web-slinging" |
        // 702.189
        "firebending" |
        // 702.190
        "sneak" |
        // 702.191
        "increment" |
        // 702.192
        "paradigm" |
        // 702.186 ∞ (Infinity)
        "\u{221e}"
    ) ||
    // ── Prefix patterns (keyword + space + cost/parameter) ───────────────────
    // 702.5 Enchant [type]
    s.starts_with("enchant ") || s == "enchant" ||
    // 702.6 Equip [cost] / Equip [quality] creature [cost]
    s.starts_with("equip") ||
    // 702.16 Protection from [quality] — colour handled above (ProtectionFrom), others ParsedUnimplemented
    // 702.21 Ward [cost] — mana cost form handled above; bare "ward" or unknown forms fall through here
    s == "ward" ||
    // 702.23 Rampage N
    s.starts_with("rampage ") ||
    // 702.24 Cumulative upkeep—[cost]  (em-dash; whole paragraph matched upstream)
    s.starts_with("cumulative upkeep") ||
    // 702.27 Buyback [cost]
    s.starts_with("buyback") ||
    // 702.30 Echo [cost]
    s.starts_with("echo ") || s == "echo" ||
    // 702.32 Fading N
    s.starts_with("fading ") ||
    // 702.33 Kicker [cost] / Multikicker [cost]
    s.starts_with("kicker") || s.starts_with("multikicker") ||
    // 702.34 Flashback [cost]
    s.starts_with("flashback") ||
    // 702.35 Madness [cost]
    s.starts_with("madness") ||
    // 702.37 Morph [cost] / Megamorph [cost]
    s.starts_with("morph") || s.starts_with("megamorph") ||
    // 702.38 Amplify N
    s.starts_with("amplify ") ||
    // 702.41 Affinity for [type]
    s.starts_with("affinity for") ||
    // 702.42 Entwine [cost]
    s.starts_with("entwine") ||
    // 702.43 Modular N
    s.starts_with("modular ") ||
    // 702.45 Bushido N
    s.starts_with("bushido ") ||
    // 702.46 Soulshift N
    s.starts_with("soulshift ") ||
    // 702.47 Splice onto [type] [cost]
    s.starts_with("splice onto") ||
    // 702.48 [Type] offering  (suffix match; type is always a single creature subtype)
    s.ends_with(" offering") ||
    // 702.49 Ninjutsu [cost]
    s.starts_with("ninjutsu") ||
    // 702.52 Dredge N
    s.starts_with("dredge ") ||
    // 702.53 Transmute [cost]
    s.starts_with("transmute") ||
    // 702.54 Bloodthirst N
    s.starts_with("bloodthirst ") ||
    // 702.56 Replicate [cost]
    s.starts_with("replicate") ||
    // 702.57 Forecast—[ability]  (em-dash; whole paragraph matched upstream)
    s.starts_with("forecast") ||
    // 702.58 Graft N
    s.starts_with("graft ") ||
    // 702.59 Recover [cost]
    s.starts_with("recover") ||
    // 702.60 Ripple N
    s.starts_with("ripple ") ||
    // 702.62 Suspend N—[cost]  (em-dash; whole paragraph matched upstream)
    s.starts_with("suspend") ||
    // 702.63 Vanishing N
    s.starts_with("vanishing ") ||
    // 702.64 Absorb N
    s.starts_with("absorb ") ||
    // 702.65 Aura swap [cost]
    s.starts_with("aura swap") ||
    // 702.67 Fortify [cost]
    s.starts_with("fortify") ||
    // 702.68 Frenzy N
    s.starts_with("frenzy ") ||
    // 702.70 Poisonous N
    s.starts_with("poisonous ") ||
    // 702.71 Transfigure [cost]
    s.starts_with("transfigure") ||
    // 702.72 Champion a [type]
    s.starts_with("champion") ||
    // 702.74 Evoke [cost]
    s.starts_with("evoke") ||
    // 702.75 Hideaway N
    s.starts_with("hideaway") ||
    // 702.76 Prowl [cost]
    s.starts_with("prowl") ||
    // 702.77 Reinforce N—[cost]  (em-dash; whole paragraph matched upstream)
    s.starts_with("reinforce") ||
    // 702.82 Devour N
    s.starts_with("devour ") ||
    // 702.84 Unearth [cost]
    s.starts_with("unearth") ||
    // 702.86 Annihilator N
    s.starts_with("annihilator ") ||
    // 702.87 Level up [cost]
    s.starts_with("level up") ||
    // 702.94 Miracle [cost]
    s.starts_with("miracle") ||
    // 702.96 Overload [cost]
    s.starts_with("overload") ||
    // 702.97 Scavenge [cost]
    s.starts_with("scavenge") ||
    // 702.103 Bestow [cost]
    s.starts_with("bestow") ||
    // 702.104 Tribute N
    s.starts_with("tribute ") ||
    // 702.107 Outlast [cost]
    s.starts_with("outlast") ||
    // 702.109 Dash [cost]
    s.starts_with("dash") ||
    // 702.112 Renown N
    s.starts_with("renown ") ||
    // 702.113 Awaken N—[cost]  (em-dash; whole paragraph matched upstream)
    s.starts_with("awaken") ||
    // 702.117 Surge [cost]
    s.starts_with("surge") ||
    // 702.119 Emerge [cost]
    s.starts_with("emerge") ||
    // 702.120 Escalate [cost]
    s.starts_with("escalate") ||
    // 702.122 Crew N
    s.starts_with("crew ") ||
    // 702.123 Fabricate N
    s.starts_with("fabricate ") ||
    // 702.124 Partner with [name]
    s.starts_with("partner with") ||
    // 702.128 Embalm [cost]
    s.starts_with("embalm") ||
    // 702.129 Eternalize [cost]
    s.starts_with("eternalize") ||
    // 702.130 Afflict N
    s.starts_with("afflict ") ||
    // 702.137 Spectacle [cost]
    s.starts_with("spectacle") ||
    // 702.138 Escape—[cost]  (em-dash; whole paragraph matched upstream)
    s.starts_with("escape") ||
    // 702.140 Mutate [cost]
    s.starts_with("mutate") ||
    // 702.141 Encore [cost]
    s.starts_with("encore") ||
    // 702.142 Boast—[ability]  (em-dash; whole paragraph matched upstream)
    s.starts_with("boast") ||
    // 702.143 Foretell [cost]
    s.starts_with("foretell") ||
    // 702.146 Disturb [cost]
    s.starts_with("disturb") ||
    // 702.148 Cleave [cost]
    s.starts_with("cleave") ||
    // 702.151 Reconfigure [cost]
    s.starts_with("reconfigure") ||
    // 702.152 Blitz [cost]
    s.starts_with("blitz") ||
    // 702.153 Casualty N
    s.starts_with("casualty ") ||
    // 702.157 Squad [cost]
    s.starts_with("squad") ||
    // 702.160 Prototype [cost] N/N
    s.starts_with("prototype") ||
    // 702.162 More Than Meets the Eye [cost]
    s.starts_with("more than meets the eye") ||
    // 702.164 Toxic N — promoted to KeywordAbility::ToxicN
    // 702.165 Backup N
    s.starts_with("backup ") ||
    // 702.167 Craft with [description] [cost]
    s.starts_with("craft with") ||
    // 702.168 Disguise [cost]
    s.starts_with("disguise") ||
    // 702.170 Plot [cost]
    s.starts_with("plot") ||
    // 702.171 Saddle N
    s.starts_with("saddle ") ||
    // 702.173 Freerunning [cost]
    s.starts_with("freerunning") ||
    // 702.174 Gift [noun]
    s.starts_with("gift") ||
    // 702.175 Offspring [cost]
    s.starts_with("offspring") ||
    // 702.176 Impending N—[cost]  (em-dash; whole paragraph matched upstream)
    s.starts_with("impending") ||
    // 702.185 Warp [cost]
    s.starts_with("warp") ||
    // ── Suffix / compound patterns ────────────────────────────────────────────
    // 702.14 Landwalk — fully handled above (promoted to KeywordAbility::Landwalk)
    // 702.29 Typecycling: mountaincycling {2}, basic landcycling {2}, etc.
    // Split on '{' so cost is stripped before checking the suffix.
    kw_part.ends_with("cycling") ||
    // 702.48 [Type] offering: goblin offering, elf offering, etc.
    s.ends_with(" offering") ||
    // 702.22 Bands with other [type]
    s.starts_with("bands with other")
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Parses "+N/+M" (or "-N/+M" etc.) returning `(power, toughness)`.
/// The first character must be `+` or `-`.
fn parse_pt_modifier_str(s: &str) -> Option<(i32, i32)> {
    let s = s.trim();
    if !s.starts_with('+') && !s.starts_with('-') {
        return None;
    }
    let slash = s.find('/')?;
    let parse_side = |side: &str| -> Option<i32> {
        let side = side.trim();
        if let Some(rest) = side.strip_prefix('+') {
            rest.parse().ok()
        } else {
            side.parse().ok()
        }
    };
    let power = parse_side(&s[..slash])?;
    let toughness = parse_side(&s[slash + 1..])?;
    Some((power, toughness))
}

/// CR 611.3b: try to parse a paragraph as a continuous P/T effect of the form
/// "Creatures you control get +N/+M.",
/// "[Color] creatures get +N/+M.", or
/// "Creatures you control with [Subtype] get +N/+M."
fn try_parse_continuous_pt_effect(paragraph: &str) -> Option<RulesText> {
    use crate::types::ability::{ContinuousEffect, ControllerFilter, PermanentFilter};
    use crate::types::card::CardType;
    use crate::types::permanent::PTDelta;

    let s = paragraph.trim_end_matches('.').trim();
    let lower = s.to_lowercase();

    // Must contain " get " separating subject from predicate.
    let get_pat = " get ";
    let get_pos = lower.find(get_pat)?;
    let subject = lower[..get_pos].trim();
    let pt_str = s[get_pos + get_pat.len()..].trim();
    let (power, toughness) = parse_pt_modifier_str(pt_str)?;

    // "creatures you control"
    if subject == "creatures you control" {
        return Some(RulesText::Active(Rule::Continuous(ContinuousEffect {
            subject_filter: PermanentFilter {
                controller: ControllerFilter::You,
                card_types: vec![CardType::Creature],
                ..Default::default()
            },
            pt_modification: Some(PTDelta { power, toughness }),
        })));
    }

    // "creatures you control with [subtype]"
    if let Some(rest) = subject.strip_prefix("creatures you control with ") {
        let sub = rest.trim();
        if !sub.is_empty() {
            let mut chars = sub.chars();
            let capitalized = chars.next()?.to_uppercase().collect::<String>() + chars.as_str();
            return Some(RulesText::Active(Rule::Continuous(ContinuousEffect {
                subject_filter: PermanentFilter {
                    controller: ControllerFilter::You,
                    card_types: vec![CardType::Creature],
                    subtypes: vec![capitalized],
                    ..Default::default()
                },
                pt_modification: Some(PTDelta { power, toughness }),
            })));
        }
    }

    // "[color] creatures"
    if let Some(color_word) = subject.strip_suffix(" creatures") {
        let color = match color_word.trim() {
            "white" => Some(ManaColor::White),
            "blue" => Some(ManaColor::Blue),
            "black" => Some(ManaColor::Black),
            "red" => Some(ManaColor::Red),
            "green" => Some(ManaColor::Green),
            _ => None,
        };
        if let Some(c) = color {
            return Some(RulesText::Active(Rule::Continuous(ContinuousEffect {
                subject_filter: PermanentFilter {
                    controller: ControllerFilter::Any,
                    card_types: vec![CardType::Creature],
                    colors: vec![c],
                    ..Default::default()
                },
                pt_modification: Some(PTDelta { power, toughness }),
            })));
        }
    }

    None
}

/// Tries to parse a paragraph as a "When/Whenever [this|CardName] enters…, effect" ETB trigger.
/// Returns `Some(Active(Triggered(...)))` on success, `Some(ParsedUnimplemented)` if the
/// trigger clause was recognised but the effect wasn't, or `None` if it's not an ETB at all.
fn try_parse_etb_trigger(paragraph: &str, card_name: &str) -> Option<RulesText> {
    use crate::types::ability::{
        Rule, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility,
    };

    // Strip "When " or "Whenever " prefix (case-insensitive).
    let lower = paragraph.to_lowercase();
    let rest: &str = if lower.starts_with("when ") {
        &paragraph[5..]
    } else if lower.starts_with("whenever ") {
        &paragraph[9..]
    } else {
        return None;
    };
    let rest = rest.trim_start();
    let rest_lower = rest.to_lowercase();

    // Match subject: "this", "this <type>" (e.g. "this creature"), or card_name.
    let after_subject: &str = if rest_lower.starts_with("this")
        && (rest.len() == 4 || rest.as_bytes().get(4) == Some(&b' '))
    {
        // Skip "this" (4 bytes) and optional one-word type
        let after_this = rest[4..].trim_start();
        if after_this.to_lowercase().starts_with("enters") {
            after_this
        } else {
            // Skip one type word (e.g. "creature", "permanent")
            let word_end = after_this.find(' ').unwrap_or(after_this.len());
            after_this[word_end..].trim_start()
        }
    } else if !card_name.is_empty() && rest_lower.starts_with(&card_name.to_lowercase()) {
        rest[card_name.len()..].trim_start()
    } else {
        return None;
    };

    // Expect "enters" optionally followed by "the battlefield".
    let after_enters: &str = {
        let al = after_subject.to_lowercase();
        if al.starts_with("enters the battlefield") {
            &after_subject["enters the battlefield".len()..]
        } else if al.starts_with("enters") {
            &after_subject["enters".len()..]
        } else {
            return None;
        }
    };

    // Find the comma separating trigger clause from effect clause.
    let comma_pos = find_at_depth_zero(after_enters, ',')?;
    let effect_str = after_enters[comma_pos + 1..].trim();

    match parse_ability_effect(effect_str) {
        Some(effect) => Some(RulesText::Active(Rule::Triggered(TriggeredAbility {
            trigger: TriggerEvent::EntersTheBattlefield {
                subject: TriggerSubjectFilter {
                    is_self: Some(true),
                    ..Default::default()
                },
            },
            condition: None,
            target_mode: TriggerTargetMode::None,
            effect,
        }))),
        None => Some(RulesText::ParsedUnimplemented(paragraph.to_string())),
    }
}

/// Parse Oracle text into a sequence of typed spans.
///
/// Always succeeds. Separators (`\n`, `,`) are consumed; each logical token
/// becomes one span. See `RulesText` for rendering intent.
pub fn parse_permanent(text: &str, card_name: &str) -> (Vec<RulesText>, Vec<TextAnnotation>) {
    const EM_DASH: char = '\u{2014}';
    let mut spans = Vec::new();
    let mut annotations: Vec<TextAnnotation> = Vec::new();

    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }

        // Em-dash at depth 0 → ability/flavour word line, or keyword with em-dash cost.
        if let Some(dash_pos) = find_at_depth_zero(paragraph, EM_DASH) {
            let left = paragraph[..dash_pos].trim();
            let right = paragraph[dash_pos + EM_DASH.len_utf8()..].trim();

            // Ward—Pay N life. (CR 702.21a)
            // Ward is a triggered ability (CR 702.21a): emitted as TriggeredAbility { trigger: TargetedBy }.
            let left_lower = left.to_lowercase();
            if left_lower == "ward" {
                let right_lower = right.to_lowercase();
                let life_str = right_lower
                    .strip_prefix("pay ")
                    .and_then(|s| s.trim_end_matches('.').strip_suffix(" life"));
                if let Some(n_str) = life_str
                    && let Some(n) = parse_number_word(n_str.trim())
                {
                    use crate::types::ability::{
                        CostComponent, TriggerEvent, TriggerTargetMode, TriggeredAbility, TurnOwner,
                    };
                    let components = vec![CostComponent::PayLife(n)];
                    let para_start = subslice_offset(text, paragraph);
                    annotations.push(TextAnnotation {
                        start: para_start,
                        end: para_start + paragraph.len(),
                        kind: AnnotationKind::Active,
                    });
                    spans.push(RulesText::Active(Rule::Triggered(TriggeredAbility {
                        trigger: TriggerEvent::TargetedBy {
                            controller: TurnOwner::Opponent,
                        },
                        condition: None,
                        target_mode: TriggerTargetMode::None,
                        effect: vec![EffectStep::Payment {
                            cost: components,
                            on_paid: vec![],
                            on_declined: vec![EffectStep::CounterSpell],
                        }],
                    })));
                    continue;
                }
                // Unrecognized Ward—... form falls through to normal em-dash handling
            }

            match match_keyword(left) {
                RulesText::ParsedUnimplemented(_) => {
                    // CR 702 keyword with em-dash syntax (Cumulative upkeep, Suspend, etc.).
                    // Emit the whole paragraph as a single recognised-unimplemented span.
                    let para_start = subslice_offset(text, paragraph);
                    annotations.push(TextAnnotation {
                        start: para_start,
                        end: para_start + paragraph.len(),
                        kind: AnnotationKind::ParsedUnimplemented,
                    });
                    spans.push(ParsedUnimplemented(paragraph.to_string()));
                    continue;
                }
                RulesText::Active(_) => {
                    // Fully-implemented keyword with em-dash — fall through to comma splitting.
                }
                _ => {
                    // Rule word or flavour word — emit label + right side.
                    let label_slice = &paragraph[..dash_pos + EM_DASH.len_utf8()];
                    let label_start = subslice_offset(text, label_slice);
                    annotations.push(TextAnnotation {
                        start: label_start,
                        end: label_start + label_slice.len(),
                        kind: AnnotationKind::AbilityWord,
                    });
                    spans.push(RulesText::Ignored(
                        IgnoredKind::AbilityWord,
                        label_slice.to_string(),
                    ));
                    if !right.is_empty() {
                        let right_start = subslice_offset(text, right);
                        annotations.push(TextAnnotation {
                            start: right_start,
                            end: right_start + right.len(),
                            kind: AnnotationKind::Unparsed,
                        });
                        spans.push(RulesText::Unparsed(right.to_string()));
                    }
                    continue;
                }
            }
        }

        // Colon check: activated ability ({cost}: effect).
        if let Some(colon_pos) = find_colon_at_depth_zero(paragraph) {
            let cost_str = paragraph[..colon_pos].trim();
            let effect_str = paragraph[colon_pos + 1..].trim();
            let cost = parse_activation_cost(cost_str);
            if !cost.is_empty() {
                let has_unimplemented_cost = cost
                    .iter()
                    .any(|c| matches!(c, CostComponent::Unimplemented(_)));
                if let Some(effect) = parse_ability_effect(effect_str) {
                    let para_start = subslice_offset(text, paragraph);
                    let ann_kind = if has_unimplemented_cost {
                        AnnotationKind::ParsedUnimplemented
                    } else {
                        AnnotationKind::Active
                    };
                    annotations.push(TextAnnotation {
                        start: para_start,
                        end: para_start + paragraph.len(),
                        kind: ann_kind,
                    });
                    spans.push(RulesText::Active(Rule::Activated(ActivatedAbility {
                        cost,
                        target_requirements: vec![],
                        effect,
                    })));
                } else {
                    let para_start = subslice_offset(text, paragraph);
                    annotations.push(TextAnnotation {
                        start: para_start,
                        end: para_start + paragraph.len(),
                        kind: AnnotationKind::ParsedUnimplemented,
                    });
                    spans.push(RulesText::ParsedUnimplemented(paragraph.to_string()));
                }
                continue;
            }
        }

        // ETB trigger check: "When/Whenever this enters…" or "When <CardName> enters…"
        if let Some(span) = try_parse_etb_trigger(paragraph, card_name) {
            let para_start = subslice_offset(text, paragraph);
            let ann_kind = match &span {
                RulesText::ParsedUnimplemented(_) => AnnotationKind::ParsedUnimplemented,
                _ => AnnotationKind::Active,
            };
            annotations.push(TextAnnotation {
                start: para_start,
                end: para_start + paragraph.len(),
                kind: ann_kind,
            });
            spans.push(span);
            continue;
        }

        // Continuous P/T effect: "Creatures you control get +N/+M." etc. (CR 611.3b)
        if let Some(span) = try_parse_continuous_pt_effect(paragraph) {
            let para_start = subslice_offset(text, paragraph);
            annotations.push(TextAnnotation {
                start: para_start,
                end: para_start + paragraph.len(),
                kind: AnnotationKind::Active,
            });
            spans.push(span);
            continue;
        }

        // Split on commas at depth 0; classify each token.
        // Track annotation count before this paragraph so we can coalesce
        // multiple Unparsed annotations from the same paragraph into one.
        let ann_before = annotations.len();
        for token in split_at_depth_zero(paragraph, ',') {
            let token = token.trim();
            if !token.is_empty() {
                emit_token_spans(token, text, &mut spans, &mut annotations);
            }
        }
        // Coalesce: if every new annotation from this paragraph is Unparsed,
        // replace them with a single annotation spanning the whole paragraph.
        let new_anns = &annotations[ann_before..];
        if new_anns.len() > 1 && new_anns.iter().all(|a| a.kind == AnnotationKind::Unparsed) {
            let para_start = subslice_offset(text, paragraph);
            let para_end = para_start + paragraph.len();
            annotations.truncate(ann_before);
            annotations.push(TextAnnotation {
                start: para_start,
                end: para_end,
                kind: AnnotationKind::Unparsed,
            });
        }
    }

    (spans, annotations)
}

/// Try to parse a "counter target [color] [type] spell [restrictions]" paragraph.
/// Returns None if the paragraph isn't a counter pattern. (CR 701.5)
///
/// Handles:
///   - bare type: "counter target spell"
///   - typed: "counter target creature spell", "counter target noncreature spell",
///     "counter target instant or sorcery spell"
///   - color prefix: "counter target red or green spell",
///     "counter target blue spell"
///   - mana-value suffix: "counter target spell with mana value 4 or greater"
///     "counter target spell with mana value 3 or less"
fn try_parse_counter(lc: &str) -> Option<crate::types::ability::SpellAbility> {
    use crate::types::ability::{SpellAbility, SpellFilter, TargetFilter};
    use crate::types::effect::EffectStep;
    use crate::types::mana::ManaColor;

    // Must start with "counter target "
    let rest = lc.strip_prefix("counter target ")?;

    // 1. Try color prefix: "[color] spell" or "[color] or [color] spell"
    let color_names: &[(&str, ManaColor)] = &[
        ("white", ManaColor::White),
        ("blue", ManaColor::Blue),
        ("black", ManaColor::Black),
        ("red", ManaColor::Red),
        ("green", ManaColor::Green),
    ];

    let mut colors: Vec<ManaColor> = Vec::new();
    let rest = {
        let mut matched_rest = rest;
        'outer: for (name1, c1) in color_names {
            // Try "color1 or color2 spell"
            let color_or_prefix = format!("{name1} or ");
            if let Some(after_c1) = rest.strip_prefix(color_or_prefix.as_str()) {
                for (name2, c2) in color_names {
                    let type_prefix = format!("{name2} spell");
                    if after_c1.starts_with(type_prefix.as_str()) {
                        colors = vec![*c1, *c2];
                        matched_rest = &after_c1[name2.len() + 1..]; // skip "[name2] "
                        break 'outer;
                    }
                }
            }
            // Try "color1 spell"
            let single_prefix = format!("{name1} ");
            if let Some(after_c1) = rest.strip_prefix(single_prefix.as_str())
                && after_c1.starts_with("spell")
            {
                colors = vec![*c1];
                matched_rest = after_c1;
                break 'outer;
            }
        }
        matched_rest
    };

    // 2. Parse type word: "instant or sorcery spell", "noncreature spell",
    //    "creature spell", "spell"
    let (base_filter, rest) = if let Some(r) = rest.strip_prefix("instant or sorcery spell") {
        (SpellFilter::instant_or_sorcery(), r)
    } else if let Some(r) = rest.strip_prefix("noncreature spell") {
        (SpellFilter::noncreature(), r)
    } else if let Some(r) = rest.strip_prefix("creature spell") {
        (SpellFilter::creature(), r)
    } else if let Some(r) = rest.strip_prefix("spell") {
        (SpellFilter::any(), r)
    } else {
        return None; // unrecognised type
    };

    let rest = rest.trim();

    // 3. Parse "with mana value N or greater/less" suffix
    let (rest, min_mv, max_mv) = parse_mana_value_suffix(rest);

    let rest = rest.trim();

    // 4. Parse "unless its controller pays {N}/{X}" or "unless its controller pays N life"
    let (rest, payment_cost) = parse_unless_suffix(rest);

    // 5. Anything left (e.g. ". Scry 2.") is additional effect text that
    // happens regardless of the counter/payment outcome (CR 608.2b).
    // Reminder text has no rules effect (CR 207.2) and is stripped first.
    let rest = rest.trim().trim_start_matches('.').trim();
    let rest = strip_reminder_text(rest);
    let rest = rest.trim();

    let filter = SpellFilter {
        any_of_colors: colors,
        min_mana_value: min_mv,
        max_mana_value: max_mv,
        ..base_filter
    };

    let mut steps = if let Some(cost) = payment_cost {
        vec![EffectStep::Payment {
            cost,
            on_paid: vec![],
            on_declined: vec![EffectStep::CounterSpell],
        }]
    } else {
        vec![EffectStep::CounterSpell]
    };
    if !rest.is_empty() {
        steps.extend(parse_spell_effect(rest));
    }

    Some(SpellAbility {
        target_requirements: vec![TargetFilter::Spell(filter)],
        steps,
    })
}

/// Strip "with mana value N or greater" / "with mana value N or less".
/// Returns (remaining, min_mana_value, max_mana_value). (CR 202.3)
fn parse_mana_value_suffix(s: &str) -> (&str, Option<u32>, Option<u32>) {
    if let Some(rest) = s.strip_prefix("with mana value ") {
        if let Some(rest) = rest.strip_suffix(" or greater")
            && let Ok(n) = rest.parse::<u32>()
        {
            return ("", Some(n), None);
        }
        if let Some(rest) = rest.strip_suffix(" or less")
            && let Ok(n) = rest.parse::<u32>()
        {
            return ("", None, Some(n));
        }
    }
    (s, None, None)
}

/// Strip "unless its controller pays {N}", "unless its controller pays {X}",
/// or "unless its controller pays N life". Only the cost token itself is
/// consumed; any trailing text (e.g. ". Scry 2.") is returned as remainder
/// rather than forcing it to be the end of the string.
/// Returns (remaining_string, Some(cost_components)) or (original, None). (CR 118.12)
fn parse_unless_suffix(s: &str) -> (&str, Option<crate::types::ability::Cost>) {
    use crate::types::ability::CostComponent;
    use crate::types::mana::{ManaCost, ManaPip};

    const PREFIX: &str = "unless its controller pays ";
    let Some(tail) = s.strip_prefix(PREFIX) else {
        return (s, None);
    };
    // Try "{N}" or "{X}" mana cost
    if let Some(inner) = tail.strip_prefix('{')
        && let Some(brace_end) = inner.find('}')
    {
        let pip_str = &inner[..brace_end];
        let pip = if pip_str.eq_ignore_ascii_case("x") {
            Some(ManaPip::X)
        } else {
            pip_str.parse::<u32>().ok().map(ManaPip::Generic)
        };
        if let Some(pip) = pip {
            return (
                &inner[brace_end + 1..],
                Some(vec![CostComponent::Mana(ManaCost { pips: vec![pip] })]),
            );
        }
    }
    // Try "N life"
    let digit_end = tail.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
    if digit_end > 0
        && let Ok(n) = tail[..digit_end].parse::<u32>()
        && let Some(remainder) = tail[digit_end..].strip_prefix(" life")
    {
        return (remainder, Some(vec![CostComponent::PayLife(n)]));
    }
    (s, None) // unrecognised unless suffix — leave as-is
}

/// Parses "put [a|N] [+1/+1|-1/-1] counter[s] on target [creature|player]" (CR 122.1).
fn try_parse_add_counter_on_target(lc: &str) -> Option<crate::types::ability::SpellAbility> {
    use crate::types::ability::{SpellAbility, TargetFilter};
    use crate::types::counter::CounterKind;
    use crate::types::effect::EffectStep;

    let rest = lc.strip_prefix("put ")?;

    let (count, rest) = if let Some(r) = rest.strip_prefix("a ") {
        (1u32, r)
    } else {
        let space = rest.find(' ')?;
        let n = parse_number_word(&rest[..space])?;
        (n, &rest[space + 1..])
    };

    let (kind, rest) = if let Some(r) = rest.strip_prefix("+1/+1 counters ") {
        (
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            r,
        )
    } else if let Some(r) = rest.strip_prefix("+1/+1 counter ") {
        (
            CounterKind::PtModifier {
                power: 1,
                toughness: 1,
            },
            r,
        )
    } else if let Some(r) = rest.strip_prefix("-1/-1 counters ") {
        (
            CounterKind::PtModifier {
                power: -1,
                toughness: -1,
            },
            r,
        )
    } else if let Some(r) = rest.strip_prefix("-1/-1 counter ") {
        (
            CounterKind::PtModifier {
                power: -1,
                toughness: -1,
            },
            r,
        )
    } else {
        return None;
    };

    let rest = rest.strip_prefix("on target ")?;

    let filter = if rest.starts_with("creature") {
        TargetFilter::Creature
    } else if rest.starts_with("player") {
        TargetFilter::Player
    } else {
        return None;
    };

    Some(SpellAbility {
        target_requirements: vec![filter],
        steps: vec![EffectStep::AddCounter { kind, count }],
    })
}

/// Detects targeting patterns in a spell paragraph and returns a SpellAbility.
///
/// Pattern A (target at front): "Target creature ..." → Creature filter; strip prefix.
/// Pattern B (card name damage): "CardName deals N damage to any target" → Any filter.
///
/// All prefix/suffix lengths are computed on the lowercase form then applied at the
/// same byte offset on the original because every prefix/suffix is pure ASCII.
fn parse_spell_paragraph(paragraph: &str, card_name: &str) -> crate::types::ability::SpellAbility {
    use crate::types::ability::{SpellAbility, TargetFilter};
    let lc = paragraph.trim_end_matches('.').to_lowercase();

    // Pattern A — "target creature " prefix
    {
        const PREFIX: &str = "target creature ";
        if lc.starts_with(PREFIX) {
            let effective = paragraph[PREFIX.len()..].trim_end_matches('.');
            let steps = parse_spell_effect(effective);
            return SpellAbility {
                target_requirements: vec![TargetFilter::Creature],
                steps,
            };
        }
    }
    // Pattern A — "target player " prefix
    {
        const PREFIX: &str = "target player ";
        if lc.starts_with(PREFIX) {
            let effective = paragraph[PREFIX.len()..].trim_end_matches('.');
            let steps = parse_spell_effect(effective);
            return SpellAbility {
                target_requirements: vec![TargetFilter::Player],
                steps,
            };
        }
    }
    // Pattern B — "<CardName> deals N damage to any target"
    {
        let card_lower = card_name.to_lowercase();
        let prefix = format!("{} ", card_lower);
        if !card_lower.is_empty() && lc.starts_with(prefix.as_str()) {
            let rest_lc = &lc[prefix.len()..];
            if let Some(damage_part) = rest_lc.strip_suffix(" to any target") {
                let steps = parse_spell_effect(damage_part);
                return SpellAbility {
                    target_requirements: vec![TargetFilter::Any],
                    steps,
                };
            }
            if let Some(damage_part) = rest_lc.strip_suffix(" to target creature") {
                let steps = parse_spell_effect(damage_part);
                return SpellAbility {
                    target_requirements: vec![TargetFilter::Creature],
                    steps,
                };
            }
        }
    }
    // Counter patterns — CR 701.5
    if let Some(spell_ability) = try_parse_counter(lc.as_str()) {
        return spell_ability;
    }
    // Counter-placement patterns — CR 122.1
    if let Some(spell_ability) = try_parse_add_counter_on_target(lc.as_str()) {
        return spell_ability;
    }
    // No targeting pattern found — untargeted spell
    SpellAbility {
        target_requirements: vec![],
        steps: parse_spell_effect(paragraph),
    }
}

/// Keyword actions (CR 701) that may appear in spell/ability text but have
/// no rules enforcement yet. Each is followed by a number, e.g. "scry 2".
const SPELL_KEYWORD_ACTIONS: &[&str] = &[
    "scry",
    "surveil",
    "fateseal",
    "amass",
    "explore",
    "investigate",
    "bolster",
    "support",
];

/// Emits `TextAnnotation`s for a single instant/sorcery paragraph: `(...)`
/// reminder text, and known-but-unimplemented keyword actions like "Scry N".
/// `paragraph` must be a subslice of `full_text` (for offset computation).
fn annotate_spell_paragraph(
    paragraph: &str,
    full_text: &str,
    annotations: &mut Vec<TextAnnotation>,
) {
    // Partition into alternating non-paren / paren segments at depth zero.
    let mut segments: Vec<(bool, &str)> = Vec::new();
    let mut depth = 0usize;
    let mut seg_start = 0usize;
    for (i, c) in paragraph.char_indices() {
        match c {
            '(' if depth == 0 => {
                if i > seg_start {
                    segments.push((false, &paragraph[seg_start..i]));
                }
                seg_start = i;
                depth = 1;
            }
            '(' => depth += 1,
            ')' if depth == 1 => {
                depth = 0;
                let end = i + ')'.len_utf8();
                segments.push((true, &paragraph[seg_start..end]));
                seg_start = end;
            }
            ')' if depth > 0 => depth -= 1,
            _ => {}
        }
    }
    if seg_start < paragraph.len() {
        segments.push((false, &paragraph[seg_start..]));
    }

    for (is_paren, seg) in segments {
        let off = subslice_offset(full_text, seg);
        if is_paren {
            annotations.push(TextAnnotation {
                start: off,
                end: off + seg.len(),
                kind: AnnotationKind::ReminderText,
            });
            continue;
        }
        // Scan sentences in this non-paren segment for known keyword actions.
        for sentence in seg.split_inclusive('.') {
            let trimmed = sentence.trim();
            if trimmed.is_empty() {
                continue;
            }
            let trimmed = trimmed.trim_end_matches('.');
            let lc = trimmed.to_lowercase();
            let matches_keyword_action = SPELL_KEYWORD_ACTIONS.iter().any(|kw| {
                lc.strip_prefix(kw)
                    .and_then(|rest| rest.strip_prefix(' '))
                    .is_some_and(|n| parse_number_word(n.trim()).is_some())
            });
            if matches_keyword_action {
                let start = subslice_offset(full_text, trimmed);
                annotations.push(TextAnnotation {
                    start,
                    end: start + trimmed.len(),
                    kind: AnnotationKind::ParsedUnimplemented,
                });
            }
        }
    }
}

/// Parse the oracle text of an instant or sorcery.
/// Each paragraph becomes one SpellAbility span containing parsed and
/// unimplemented effect steps in written order (CR 609).
pub fn parse_instant_or_sorcery(
    text: &str,
    card_name: &str,
) -> (Vec<RulesText>, Vec<TextAnnotation>) {
    use crate::types::ability::Rule;
    let mut spans = Vec::new();
    let mut annotations = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        let spell_ability = parse_spell_paragraph(paragraph, card_name);
        spans.push(RulesText::Active(Rule::SpellAbility(spell_ability)));
        annotate_spell_paragraph(paragraph, text, &mut annotations);
    }
    (spans, annotations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::KeywordAbility;

    fn parse_perm(text: &str, name: &str) -> Vec<RulesText> {
        parse_permanent(text, name).0
    }
    fn parse_spell(text: &str, name: &str) -> Vec<RulesText> {
        parse_instant_or_sorcery(text, name).0
    }

    fn active(kw: KeywordAbility) -> RulesText {
        RulesText::Active(Rule::Static(kw))
    }
    fn reminder(text: &str) -> RulesText {
        RulesText::Ignored(IgnoredKind::ReminderText, text.to_string())
    }
    fn ability_word(text: &str) -> RulesText {
        RulesText::Ignored(IgnoredKind::AbilityWord, text.to_string())
    }
    fn unparsed(text: &str) -> RulesText {
        RulesText::Unparsed(text.to_string())
    }
    fn unimplemented(text: &str) -> RulesText {
        RulesText::ParsedUnimplemented(text.to_string())
    }

    #[test]
    fn empty_text_returns_empty_vec() {
        assert_eq!(parse_perm("", ""), vec![]);
    }

    #[test]
    fn blank_lines_skipped() {
        assert_eq!(parse_perm("\n\n", ""), vec![]);
    }

    #[test]
    fn reminder_text_only() {
        assert_eq!(
            parse_perm("({T}: Add {G}.)", ""),
            vec![reminder("({T}: Add {G}.)")]
        );
    }

    #[test]
    fn single_keyword() {
        assert_eq!(
            parse_perm("Flying", ""),
            vec![active(KeywordAbility::Flying)]
        );
    }

    #[test]
    fn comma_separated_keywords() {
        assert_eq!(
            parse_perm("Flying, vigilance", ""),
            vec![
                active(KeywordAbility::Flying),
                active(KeywordAbility::Vigilance)
            ]
        );
    }

    #[test]
    fn multiline_keywords() {
        assert_eq!(
            parse_perm("Trample\nLifelink", ""),
            vec![
                active(KeywordAbility::Trample),
                active(KeywordAbility::Lifelink)
            ]
        );
    }

    #[test]
    fn two_word_keyword() {
        assert_eq!(
            parse_perm("First strike", ""),
            vec![active(KeywordAbility::FirstStrike)]
        );
    }

    #[test]
    fn keyword_with_reminder_text() {
        assert_eq!(
            parse_perm(
                "Deathtouch (Any amount of damage this deals to a creature is enough to destroy it.)",
                "",
            ),
            vec![
                active(KeywordAbility::Deathtouch),
                reminder(
                    "(Any amount of damage this deals to a creature is enough to destroy it.)"
                ),
            ]
        );
    }

    #[test]
    fn ability_word_line_splits_at_em_dash() {
        let result = parse_perm(
            "Landfall \u{2014} Whenever a land you control enters, you gain 1 life.",
            "",
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
    fn cumulative_upkeep_emits_parsed_unimplemented() {
        let result = parse_perm("Cumulative upkeep\u{2014}Add {R}.", "");
        assert_eq!(
            result,
            vec![unimplemented("Cumulative upkeep\u{2014}Add {R}.")]
        );
    }

    #[test]
    fn em_dash_inside_parens_not_split() {
        assert_eq!(
            parse_perm("(Choose one \u{2014} do A; or do B.)", ""),
            vec![reminder("(Choose one \u{2014} do A; or do B.)")]
        );
    }

    #[test]
    fn all_implemented_keywords_parse() {
        let text = "Flying\nReach\nTrample\nFirst strike\nDouble strike\nVigilance\nHaste\nLifelink\nDeathtouch\nMenace\nIndestructible\nDefender\nShadow\nHorsemanship\nSkulk\nDecayed\nFlash";
        let result = parse_perm(text, "");
        assert_eq!(result.len(), 17);
        assert!(result.iter().all(|s| matches!(s, RulesText::Active(_))));
    }

    #[test]
    fn bare_unimplemented_keyword_emits_parsed_unimplemented() {
        assert_eq!(parse_perm("Cascade", ""), vec![unimplemented("Cascade")]);
    }

    #[test]
    fn parameterised_keyword_emits_parsed_unimplemented() {
        // Cycling {2} is now promoted to Parsed(Rule::Cycling(...))
        // Kicker {1}{U} is now promoted to Parsed(Rule::Kicker(...))
        // See parse_cycling_keyword and parse_kicker_mana_cost tests for their new behavior.
        // Test with a keyword that remains unimplemented.
        assert_eq!(
            parse_perm("Madness {1}{U}", ""),
            vec![unimplemented("Madness {1}{U}")]
        );
    }

    #[test]
    fn landwalk_variants_parse_as_static_ability() {
        use crate::types::LandwalkKind;
        // Landwalk is now promoted to KeywordAbility::Landwalk
        assert_eq!(
            parse_perm("Islandwalk", ""),
            vec![active(KeywordAbility::Landwalk(LandwalkKind::LandType(
                "Island".to_string()
            )))]
        );
        assert_eq!(
            parse_perm("Nonbasic landwalk", ""),
            vec![active(KeywordAbility::Landwalk(LandwalkKind::Nonbasic))]
        );
    }

    #[test]
    fn typecycling_with_space_emits_parsed_unimplemented() {
        // 702.29e: "basic landcycling" has a space between the two type words
        assert_eq!(
            parse_perm("Basic landcycling {2}", ""),
            vec![unimplemented("Basic landcycling {2}")]
        );
        assert_eq!(
            parse_perm("Mountaincycling {1}", ""),
            vec![unimplemented("Mountaincycling {1}")]
        );
    }

    #[test]
    fn em_dash_keyword_emits_whole_paragraph_as_parsed_unimplemented() {
        // Suspend: "Suspend 2—{1}{U}"
        assert_eq!(
            parse_perm("Suspend 2\u{2014}{1}{U}", ""),
            vec![unimplemented("Suspend 2\u{2014}{1}{U}")]
        );
    }

    // ── parse_activation_cost / parse_ability_effect ──────────────────────────

    #[test]
    fn parse_activation_cost_tap_only() {
        use crate::types::ability::CostComponent;
        let cost = super::parse_activation_cost("{T}");
        assert_eq!(cost, vec![CostComponent::Tap]);
    }

    #[test]
    fn parse_activation_cost_mana_and_tap() {
        use crate::types::ability::CostComponent;
        use crate::types::mana::{ManaCost, ManaPip};
        let cost = super::parse_activation_cost("{2}, {T}");
        assert_eq!(
            cost,
            vec![
                CostComponent::Mana(ManaCost {
                    pips: vec![ManaPip::Generic(2)],
                }),
                CostComponent::Tap,
            ]
        );
    }

    #[test]
    fn parse_activation_cost_unrecognised_becomes_unimplemented() {
        use crate::types::ability::CostComponent;
        let cost = super::parse_activation_cost("Sacrifice a creature");
        assert_eq!(
            cost,
            vec![CostComponent::Unimplemented(
                "Sacrifice a creature".to_string()
            )]
        );
    }

    #[test]
    fn parse_ability_effect_add_mana() {
        use crate::types::effect::EffectStep;
        use crate::types::mana::ManaPool;
        let effect = super::parse_ability_effect("Add {G}.").unwrap();
        assert_eq!(
            effect,
            vec![EffectStep::AddMana(ManaPool {
                green: 1,
                ..Default::default()
            })]
        );
    }

    #[test]
    fn parse_ability_effect_draw_a_card() {
        use crate::types::effect::EffectStep;
        let effect = super::parse_ability_effect("Draw a card.").unwrap();
        assert_eq!(effect, vec![EffectStep::DrawCard(1)]);
    }

    #[test]
    fn parse_ability_effect_mill_two() {
        use crate::types::effect::EffectStep;
        let effect = super::parse_ability_effect("Mill 2.").unwrap();
        assert_eq!(effect, vec![EffectStep::Mill(2)]);
    }

    #[test]
    fn parse_ability_effect_multi_step() {
        use crate::types::effect::EffectStep;
        let effect = super::parse_ability_effect("Mill 2. Draw a card.").unwrap();
        assert_eq!(effect, vec![EffectStep::Mill(2), EffectStep::DrawCard(1),]);
    }

    #[test]
    fn parse_ability_effect_unknown_returns_none() {
        assert!(super::parse_ability_effect("Create a 1/1 token.").is_none());
    }

    // ── try_parse_mana_cost / try_parse_mana_pool / parse_number_word ─────────

    #[test]
    fn try_parse_mana_cost_single_color() {
        use crate::types::mana::{ManaCost, ManaPip};
        let c = super::try_parse_mana_cost("{G}").unwrap();
        assert_eq!(
            c,
            ManaCost {
                pips: vec![ManaPip::Green]
            }
        );
    }

    #[test]
    fn try_parse_mana_cost_generic_and_color() {
        use crate::types::mana::{ManaCost, ManaPip};
        let c = super::try_parse_mana_cost("{2}{G}").unwrap();
        assert_eq!(
            c,
            ManaCost {
                pips: vec![ManaPip::Generic(2), ManaPip::Green]
            }
        );
    }

    #[test]
    fn try_parse_mana_cost_tap_symbol_is_none() {
        assert!(super::try_parse_mana_cost("{T}").is_none());
    }

    #[test]
    fn try_parse_mana_cost_non_symbol_text_is_none() {
        assert!(super::try_parse_mana_cost("Sacrifice a creature").is_none());
    }

    #[test]
    fn try_parse_mana_cost_hybrid() {
        use crate::types::mana::{ManaColor, ManaPip};
        let cost = super::try_parse_mana_cost("{B/G}").unwrap();
        assert_eq!(
            cost.pips,
            vec![ManaPip::Hybrid(ManaColor::Black, ManaColor::Green)]
        );
    }

    #[test]
    fn try_parse_mana_cost_phyrexian() {
        use crate::types::mana::{ManaColor, ManaPip};
        let cost = super::try_parse_mana_cost("{U/P}").unwrap();
        assert_eq!(cost.pips, vec![ManaPip::Phyrexian(ManaColor::Blue)]);
    }

    #[test]
    fn try_parse_mana_cost_x() {
        use crate::types::mana::ManaPip;
        let cost = super::try_parse_mana_cost("{X}{R}").unwrap();
        assert_eq!(cost.pips, vec![ManaPip::X, ManaPip::Red]);
    }

    #[test]
    fn try_parse_mana_cost_snow() {
        use crate::types::mana::ManaPip;
        let cost = super::try_parse_mana_cost("{S}").unwrap();
        assert_eq!(cost.pips, vec![ManaPip::Snow]);
    }

    #[test]
    fn try_parse_mana_cost_hybrid_phyrexian() {
        use crate::types::mana::{ManaColor, ManaPip};
        let cost = super::try_parse_mana_cost("{G/U/P}").unwrap();
        assert_eq!(
            cost.pips,
            vec![ManaPip::HybridPhyrexian(ManaColor::Green, ManaColor::Blue)]
        );
    }

    #[test]
    fn try_parse_mana_cost_generic_hybrid() {
        use crate::types::mana::{ManaColor, ManaPip};
        let cost = super::try_parse_mana_cost("{2/R}").unwrap();
        assert_eq!(cost.pips, vec![ManaPip::GenericHybrid(2, ManaColor::Red)]);
    }

    #[test]
    fn try_parse_mana_cost_colorless_hybrid() {
        use crate::types::mana::{ManaColor, ManaPip};
        let cost = super::try_parse_mana_cost("{C/G}").unwrap();
        assert_eq!(cost.pips, vec![ManaPip::ColorlessHybrid(ManaColor::Green)]);
    }

    #[test]
    fn try_parse_mana_cost_plain_text_is_none() {
        assert!(super::try_parse_mana_cost("Sacrifice a creature").is_none());
    }

    #[test]
    fn try_parse_mana_pool_green() {
        use crate::types::mana::ManaPool;
        let p = super::try_parse_mana_pool("{G}").unwrap();
        assert_eq!(
            p,
            ManaPool {
                green: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn try_parse_mana_pool_two_colors() {
        use crate::types::mana::ManaPool;
        let p = super::try_parse_mana_pool("{G}{W}").unwrap();
        assert_eq!(
            p,
            ManaPool {
                green: 1,
                white: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn try_parse_mana_pool_generic_is_none() {
        assert!(super::try_parse_mana_pool("{2}").is_none());
    }

    #[test]
    fn parse_number_word_digits_and_words() {
        assert_eq!(super::parse_number_word("2"), Some(2));
        assert_eq!(super::parse_number_word("two"), Some(2));
        assert_eq!(super::parse_number_word("three"), Some(3));
        assert_eq!(super::parse_number_word("banana"), None);
    }

    // ── find_colon_at_depth_zero ──────────────────────────────────────────────

    #[test]
    fn find_colon_none_for_no_colon() {
        assert_eq!(super::find_colon_at_depth_zero("Flying"), None);
    }

    #[test]
    fn find_colon_skips_inside_parens() {
        assert_eq!(super::find_colon_at_depth_zero("({T}: Add {G}.)"), None);
    }

    #[test]
    fn find_colon_skips_inside_braces() {
        // hypothetical, but verifies brace depth tracking
        assert_eq!(super::find_colon_at_depth_zero("{T}: Add {G}."), Some(3));
    }

    #[test]
    fn find_colon_at_depth_zero_comma_cost() {
        // "{2}, {T}: Add {G}." — colon is at index 8
        assert_eq!(
            super::find_colon_at_depth_zero("{2}, {T}: Add {G}."),
            Some(8)
        );
    }

    #[test]
    fn keyword_and_ability_word_on_separate_lines() {
        let text = "Flying\nLandfall \u{2014} Whenever a land you control enters, you gain 1 life.";
        let result = parse_perm(text, "");
        assert_eq!(
            result,
            vec![
                active(KeywordAbility::Flying),
                ability_word("Landfall \u{2014}"),
                unparsed("Whenever a land you control enters, you gain 1 life."),
            ]
        );
    }

    // ── Activated ability integration tests ──────────────────────────────────

    #[test]
    fn tap_add_green_parses_as_activated() {
        use crate::types::ability::{ActivatedAbility, CostComponent};
        use crate::types::effect::EffectStep;
        use crate::types::mana::ManaPool;
        let result = parse_perm("{T}: Add {G}.", "");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            RulesText::Active(Rule::Activated(ActivatedAbility {
                cost,
                effect,
                ..
            })) if cost == &vec![CostComponent::Tap]
                && effect == &vec![EffectStep::AddMana(ManaPool { green: 1, ..Default::default() })]
        ));
    }

    #[test]
    fn two_tap_add_two_green_parses_as_activated() {
        use crate::types::ability::CostComponent;
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip, ManaPool};
        let result = parse_perm("{2}, {T}: Add {G}{G}.", "");
        assert_eq!(result.len(), 1);
        if let RulesText::Active(Rule::Activated(ability)) = &result[0] {
            assert_eq!(
                ability.cost,
                vec![
                    CostComponent::Mana(ManaCost {
                        pips: vec![ManaPip::Generic(2)]
                    }),
                    CostComponent::Tap,
                ]
            );
            assert_eq!(
                ability.effect,
                vec![EffectStep::AddMana(ManaPool {
                    green: 2,
                    ..Default::default()
                })]
            );
        } else {
            panic!("expected activated ability");
        }
    }

    #[test]
    fn one_draw_a_card_parses_as_activated() {
        use crate::types::ability::CostComponent;
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip};
        let result = parse_perm("{1}: Draw a card.", "");
        assert_eq!(result.len(), 1);
        if let RulesText::Active(Rule::Activated(ability)) = &result[0] {
            assert_eq!(
                ability.cost,
                vec![CostComponent::Mana(ManaCost {
                    pips: vec![ManaPip::Generic(1)]
                })]
            );
            assert_eq!(ability.effect, vec![EffectStep::DrawCard(1)]);
        } else {
            panic!("expected activated ability");
        }
    }

    #[test]
    fn tap_mill_two_parses_as_activated() {
        use crate::types::ability::{ActivatedAbility, CostComponent};
        use crate::types::effect::EffectStep;
        let result = parse_perm("{T}: Mill 2.", "");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            RulesText::Active(Rule::Activated(ActivatedAbility { cost, effect, .. }))
            if cost == &vec![CostComponent::Tap]
            && effect == &vec![EffectStep::Mill(2)]
        ));
    }

    #[test]
    fn reminder_text_colon_not_treated_as_activated() {
        // ({T}: Add {G}.) is reminder text — not an activated ability
        let result = parse_perm("({T}: Add {G}.)", "");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            RulesText::Ignored(IgnoredKind::ReminderText, _)
        ));
    }

    #[test]
    fn sacrifice_cost_becomes_unimplemented_in_cost_activated_parsed() {
        use crate::types::ability::{ActivatedAbility, CostComponent};
        use crate::types::effect::EffectStep;
        use crate::types::mana::ManaPool;
        let result = parse_perm("Sacrifice a creature: Add {G}{G}.", "");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            RulesText::Active(Rule::Activated(ActivatedAbility { cost, effect, .. }))
            if cost == &vec![CostComponent::Unimplemented("Sacrifice a creature".to_string())]
            && effect == &vec![EffectStep::AddMana(ManaPool { green: 2, ..Default::default() })]
        ));
    }

    #[test]
    fn unknown_effect_becomes_parsed_unimplemented() {
        let result = parse_perm("{T}: Create a 1/1 token.", "");
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], RulesText::ParsedUnimplemented(_)));
    }

    #[test]
    fn parse_ability_effect_gain_life() {
        use crate::types::effect::EffectStep;
        assert_eq!(
            super::parse_ability_effect("You gain 3 life."),
            Some(vec![EffectStep::GainLife(3)])
        );
        assert_eq!(
            super::parse_ability_effect("gain 1 life."),
            Some(vec![EffectStep::GainLife(1)])
        );
        assert_eq!(
            super::parse_ability_effect("you gain two life."),
            Some(vec![EffectStep::GainLife(2)])
        );
    }

    #[test]
    fn etb_self_draw_parses_as_triggered() {
        use crate::types::ability::{Rule, TriggerEvent, TriggeredAbility};
        use crate::types::effect::EffectStep;
        let result = parse_perm("When this enters, draw a card.", "");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield { subject },
                effect,
                ..
            })) if subject.is_self == Some(true) && effect == &vec![EffectStep::DrawCard(1)]
        ));
    }

    #[test]
    fn etb_creature_form_parses_as_triggered() {
        use crate::types::ability::{Rule, TriggerEvent, TriggeredAbility};
        use crate::types::effect::EffectStep;
        // Older template: "When this creature enters"
        let result = parse_perm("When this creature enters, draw a card.", "");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield { subject },
                effect,
                ..
            })) if subject.is_self == Some(true) && effect == &vec![EffectStep::DrawCard(1)]
        ));
    }

    #[test]
    fn etb_battlefield_form_parses_as_triggered() {
        use crate::types::ability::{Rule, TriggerEvent, TriggeredAbility};
        use crate::types::effect::EffectStep;
        let result = parse_perm("Whenever this enters the battlefield, you gain 3 life.", "");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield { subject },
                effect,
                ..
            })) if subject.is_self == Some(true) && effect == &vec![EffectStep::GainLife(3)]
        ));
    }

    #[test]
    fn etb_card_name_subject_parses_as_triggered() {
        use crate::types::ability::{Rule, TriggerEvent, TriggeredAbility};
        use crate::types::effect::EffectStep;
        let result = parse_perm(
            "When Elvish Visionary enters the battlefield, draw a card.",
            "Elvish Visionary",
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::EntersTheBattlefield { subject },
                effect,
                ..
            })) if subject.is_self == Some(true) && effect == &vec![EffectStep::DrawCard(1)]
        ));
    }

    #[test]
    fn etb_multistep_effect_parses_as_triggered() {
        use crate::types::ability::{Rule, TriggeredAbility};
        use crate::types::effect::EffectStep;
        let result = parse_perm("When this enters, draw a card. You gain 2 life.", "");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            RulesText::Active(Rule::Triggered(TriggeredAbility { effect, .. }))
            if effect == &vec![EffectStep::DrawCard(1), EffectStep::GainLife(2)]
        ));
    }

    #[test]
    fn etb_unknown_effect_becomes_parsed_unimplemented() {
        let result = parse_perm("When this enters, create a 1/1 token.", "");
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], RulesText::ParsedUnimplemented(_)));
    }

    // ── parse_instant_or_sorcery ─────────────────────────────────────────────────

    fn spell_effect(steps: Vec<EffectStep>) -> RulesText {
        use crate::types::ability::SpellAbility;
        RulesText::Active(Rule::SpellAbility(SpellAbility {
            target_requirements: vec![],
            steps,
        }))
    }
    fn unimpl(s: &str) -> EffectStep {
        EffectStep::Unimplemented(s.to_string())
    }

    #[test]
    fn instant_draw_one_card() {
        let result = parse_spell("Draw a card.", "");
        assert_eq!(result, vec![spell_effect(vec![EffectStep::DrawCard(1)])]);
    }

    #[test]
    fn brainstorm_then_split() {
        // ", then " splits intra-sentence; DrawCard(3) is parseable, the rest is not
        let result = parse_spell(
            "Draw three cards, then put two cards from your hand on top of your library in any order.",
            "",
        );
        assert_eq!(
            result,
            vec![spell_effect(vec![
                EffectStep::DrawCard(3),
                unimpl("put two cards from your hand on top of your library in any order"),
            ])]
        );
    }

    #[test]
    fn opt_period_then_draw() {
        // ". " splits sentences; first sentence unimplemented, second parseable
        let result = parse_spell("Scry 1. Draw a card.", "");
        assert_eq!(
            result,
            vec![spell_effect(vec![
                unimpl("Scry 1"),
                EffectStep::DrawCard(1),
            ])]
        );
    }

    #[test]
    fn serum_visions_draw_then_scry() {
        // "Draw a card, then scry 2." — draw is parsed, scry is unimplemented
        let result = parse_spell("Draw a card, then scry 2.", "");
        assert_eq!(
            result,
            vec![spell_effect(vec![
                EffectStep::DrawCard(1),
                unimpl("scry 2"),
            ])]
        );
    }

    #[test]
    fn counterspell_parses_to_counter_any_spell() {
        use crate::types::ability::{SpellFilter, TargetFilter};
        use crate::types::effect::EffectStep;
        let result = parse_spell("Counter target spell.", "");
        assert_eq!(result.len(), 1);
        let RulesText::Active(Rule::SpellAbility(sa)) = &result[0] else {
            panic!("expected SpellAbility, got {:?}", result[0]);
        };
        assert_eq!(
            sa.target_requirements,
            vec![TargetFilter::Spell(SpellFilter::any())]
        );
        assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
    }

    #[test]
    fn negate_parses_to_counter_noncreature_spell() {
        use crate::types::ability::{SpellFilter, TargetFilter};
        use crate::types::effect::EffectStep;
        let result = parse_spell("Counter target noncreature spell.", "");
        let RulesText::Active(Rule::SpellAbility(sa)) = &result[0] else {
            panic!("expected SpellAbility");
        };
        assert_eq!(
            sa.target_requirements,
            vec![TargetFilter::Spell(SpellFilter::noncreature())]
        );
        assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
    }

    #[test]
    fn essence_scatter_parses_to_counter_creature_spell() {
        use crate::types::ability::{SpellFilter, TargetFilter};
        use crate::types::effect::EffectStep;
        let result = parse_spell("Counter target creature spell.", "");
        let RulesText::Active(Rule::SpellAbility(sa)) = &result[0] else {
            panic!("expected SpellAbility");
        };
        assert_eq!(
            sa.target_requirements,
            vec![TargetFilter::Spell(SpellFilter::creature())]
        );
        assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
    }

    #[test]
    fn dispel_parses_to_counter_instant_or_sorcery_spell() {
        use crate::types::ability::{SpellFilter, TargetFilter};
        use crate::types::effect::EffectStep;
        let result = parse_spell("Counter target instant or sorcery spell.", "");
        let RulesText::Active(Rule::SpellAbility(sa)) = &result[0] else {
            panic!("expected SpellAbility");
        };
        assert_eq!(
            sa.target_requirements,
            vec![TargetFilter::Spell(SpellFilter::instant_or_sorcery())]
        );
        assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
    }

    #[test]
    fn ponder_multi_sentence_mixed() {
        // One paragraph; three sentences; first has ", then " inside it
        let result = parse_spell(
            "Look at the top three cards of your library, then put them back in any order. You may shuffle. Draw a card.",
            "",
        );
        assert_eq!(
            result,
            vec![spell_effect(vec![
                unimpl("Look at the top three cards of your library"),
                unimpl("put them back in any order"),
                unimpl("You may shuffle"),
                EffectStep::DrawCard(1),
            ])]
        );
    }

    #[test]
    fn empty_oracle_text_returns_empty() {
        let result = parse_spell("", "");
        assert_eq!(result, vec![]);
    }

    // ── Targeted spell parsing ─────────────────────────────────────────────────

    #[test]
    fn parse_giant_growth_effect() {
        use crate::types::PTDelta;
        use crate::types::ability::TargetFilter;
        use crate::types::effect::EffectStep;
        let result = parse_spell(
            "Target creature gets +3/+3 until end of turn.",
            "Giant Growth",
        );
        assert_eq!(result.len(), 1);
        let RulesText::Active(Rule::SpellAbility(sa)) = &result[0] else {
            panic!("expected SpellAbility, got {:?}", result[0]);
        };
        assert_eq!(sa.target_requirements, vec![TargetFilter::Creature]);
        assert_eq!(
            sa.steps,
            vec![EffectStep::BoostPermanentPT(PTDelta {
                power: 3,
                toughness: 3
            })]
        );
    }

    #[test]
    fn parse_lightning_bolt_effect() {
        use crate::types::ability::TargetFilter;
        use crate::types::effect::EffectStep;
        let result = parse_spell(
            "Lightning Bolt deals 3 damage to any target.",
            "Lightning Bolt",
        );
        assert_eq!(result.len(), 1);
        let RulesText::Active(Rule::SpellAbility(sa)) = &result[0] else {
            panic!("expected SpellAbility, got {:?}", result[0]);
        };
        assert_eq!(sa.target_requirements, vec![TargetFilter::Any]);
        assert_eq!(
            sa.steps,
            vec![EffectStep::DealDamage(crate::types::effect::DamageStep {
                amount: 3,
                ..Default::default()
            })]
        );
    }

    #[test]
    fn parse_draw_a_card_spell_is_untargeted() {
        let result = parse_spell("Draw a card.", "Opt");
        assert_eq!(result.len(), 1);
        let RulesText::Active(Rule::SpellAbility(sa)) = &result[0] else {
            panic!("expected SpellAbility");
        };
        assert!(sa.target_requirements.is_empty());
    }

    #[test]
    fn battlegrowth_put_counter_on_target_creature() {
        use crate::types::ability::TargetFilter;
        use crate::types::counter::CounterKind;
        use crate::types::effect::EffectStep;
        let result = parse_spell("Put a +1/+1 counter on target creature.", "Battlegrowth");
        assert_eq!(result.len(), 1);
        let RulesText::Active(Rule::SpellAbility(sa)) = &result[0] else {
            panic!("expected SpellAbility, got {:?}", result[0]);
        };
        assert_eq!(sa.target_requirements, vec![TargetFilter::Creature]);
        assert_eq!(
            sa.steps,
            vec![EffectStep::AddCounter {
                kind: CounterKind::PtModifier {
                    power: 1,
                    toughness: 1
                },
                count: 1,
            }]
        );
    }

    #[test]
    fn put_two_counters_on_target_creature() {
        use crate::types::ability::TargetFilter;
        use crate::types::counter::CounterKind;
        use crate::types::effect::EffectStep;
        let result = parse_spell("Put two +1/+1 counters on target creature.", "");
        assert_eq!(result.len(), 1);
        let RulesText::Active(Rule::SpellAbility(sa)) = &result[0] else {
            panic!("expected SpellAbility, got {:?}", result[0]);
        };
        assert_eq!(sa.target_requirements, vec![TargetFilter::Creature]);
        assert_eq!(
            sa.steps,
            vec![EffectStep::AddCounter {
                kind: CounterKind::PtModifier {
                    power: 1,
                    toughness: 1
                },
                count: 2,
            }]
        );
    }

    #[test]
    fn put_minus_counter_on_target_creature() {
        use crate::types::ability::TargetFilter;
        use crate::types::counter::CounterKind;
        use crate::types::effect::EffectStep;
        let result = parse_spell("Put a -1/-1 counter on target creature.", "");
        assert_eq!(result.len(), 1);
        let RulesText::Active(Rule::SpellAbility(sa)) = &result[0] else {
            panic!("expected SpellAbility, got {:?}", result[0]);
        };
        assert_eq!(sa.target_requirements, vec![TargetFilter::Creature]);
        assert_eq!(
            sa.steps,
            vec![EffectStep::AddCounter {
                kind: CounterKind::PtModifier {
                    power: -1,
                    toughness: -1
                },
                count: 1,
            }]
        );
    }

    #[test]
    fn flash_parses_as_static_ability() {
        let result = parse_perm("Flash", "");
        assert_eq!(
            result,
            vec![RulesText::Active(Rule::Static(KeywordAbility::Flash))]
        );
    }

    #[test]
    fn parse_exalted_keyword() {
        let spans = parse_perm("Exalted", "");
        assert_eq!(spans, vec![active(KeywordAbility::Exalted)]);
    }

    #[test]
    fn parse_flanking_keyword() {
        let spans = parse_perm("Flanking", "");
        assert_eq!(spans, vec![active(KeywordAbility::Flanking)]);
    }

    #[test]
    fn parse_bushido_n_keyword() {
        use crate::types::ability::KeywordAbility;
        let spans = parse_perm("Bushido 2", "");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Static(KeywordAbility::BushidoN(2)))]
        );
    }

    #[test]
    fn parse_wither_keyword() {
        let spans = parse_perm("Wither", "");
        assert_eq!(spans, vec![active(KeywordAbility::Wither)]);
    }

    #[test]
    fn parse_infect_keyword() {
        let spans = parse_perm("Infect", "");
        assert_eq!(spans, vec![active(KeywordAbility::Infect)]);
    }

    #[test]
    fn parse_evolve_keyword() {
        let spans = parse_perm("Evolve", "");
        assert_eq!(spans, vec![active(KeywordAbility::Evolve)]);
    }

    #[test]
    fn parse_training_keyword() {
        let spans = parse_perm("Training", "");
        assert_eq!(spans, vec![active(KeywordAbility::Training)]);
    }

    #[test]
    fn parse_toxic_n_keyword() {
        let spans = parse_perm("Toxic 3", "");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Static(KeywordAbility::ToxicN(3)))]
        );
    }

    #[test]
    fn parse_melee_keyword() {
        let spans = parse_perm("Melee", "");
        assert_eq!(spans, vec![active(KeywordAbility::Melee)]);
    }

    #[test]
    fn parse_prowess_keyword() {
        let spans = parse_perm("Prowess", "");
        assert_eq!(spans, vec![active(KeywordAbility::Prowess)]);
    }

    #[test]
    fn parse_cycling_keyword() {
        use crate::types::mana::{ManaCost, ManaPip};
        let spans = parse_perm("Cycling {2}", "");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Cycling(ManaCost {
                pips: vec![ManaPip::Generic(2)],
            }))]
        );
    }

    #[test]
    fn parse_cycling_with_reminder_text() {
        use crate::types::mana::{ManaCost, ManaPip};
        let spans = parse_perm("Cycling {2} ({2}, Discard this card: Draw a card.)", "");
        // First span is the cycling ability; second is reminder text (ignored).
        assert_eq!(spans.len(), 2);
        assert_eq!(
            spans[0],
            RulesText::Active(Rule::Cycling(ManaCost {
                pips: vec![ManaPip::Generic(2)],
            }))
        );
        assert!(matches!(
            &spans[1],
            RulesText::Ignored(crate::types::ability::IgnoredKind::ReminderText, _)
        ));
    }

    #[test]
    fn parse_cycling_colored_cost() {
        // Regression: mana symbols in cycling costs must preserve case for try_parse_mana_cost.
        use crate::types::mana::{ManaCost, ManaPip};
        let spans = parse_perm("Cycling {U}", "");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Cycling(ManaCost {
                pips: vec![ManaPip::Blue],
            }))]
        );
        let spans = parse_perm("Cycling {1}{W}", "");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Cycling(ManaCost {
                pips: vec![ManaPip::Generic(1), ManaPip::White],
            }))]
        );
    }

    #[test]
    fn mountaincycling_stays_parsed_unimplemented() {
        let spans = parse_perm("Mountaincycling {2}", "");
        assert!(matches!(&spans[0], RulesText::ParsedUnimplemented(_)));
    }

    #[test]
    fn parse_shroud_keyword() {
        use crate::types::ability::KeywordAbility;
        assert_eq!(
            parse_perm("Shroud", ""),
            vec![active(KeywordAbility::Shroud)]
        );
    }

    #[test]
    fn parse_hexproof_keyword() {
        use crate::types::ability::KeywordAbility;
        assert_eq!(
            parse_perm("Hexproof", ""),
            vec![active(KeywordAbility::Hexproof)]
        );
    }

    #[test]
    fn reminder_text_emits_annotation() {
        let text =
            "Deathtouch (Any amount of damage this deals to a creature is enough to destroy it.)";
        let (_, annotations) = parse_permanent(text, "");
        // Expect Active annotation for "Deathtouch" keyword plus ReminderText for the parens.
        assert_eq!(annotations.len(), 2);
        let active_ann = annotations
            .iter()
            .find(|a| a.kind == AnnotationKind::Active)
            .expect("expected Active annotation for implemented keyword");
        assert_eq!(active_ann.start, 0);
        assert_eq!(active_ann.end, "Deathtouch".len());
        let rt_ann = annotations
            .iter()
            .find(|a| a.kind == AnnotationKind::ReminderText)
            .expect("expected ReminderText annotation");
        let expected_start = text.find('(').unwrap();
        assert_eq!(rt_ann.start, expected_start);
        assert_eq!(rt_ann.end, text.len());
    }

    #[test]
    fn parsed_keyword_emits_active_annotation() {
        let (_, annotations) = parse_permanent("Flying", "");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].kind, AnnotationKind::Active);
    }

    #[test]
    fn parsed_unimplemented_keyword_emits_annotation() {
        let text = "Storm";
        let (_, annotations) = parse_permanent(text, "");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].kind, AnnotationKind::ParsedUnimplemented);
        assert_eq!(annotations[0].start, 0);
        assert_eq!(annotations[0].end, text.len());
    }

    #[test]
    fn unparsed_text_emits_annotation() {
        let text = "Whenever a land you control enters, you gain 1 life.";
        let (_, annotations) = parse_permanent(text, "");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].kind, AnnotationKind::Unparsed);
        assert_eq!(annotations[0].start, 0);
        assert_eq!(annotations[0].end, text.len());
    }

    #[test]
    fn ability_word_emits_ability_word_and_unparsed_annotations() {
        let text = "Landfall \u{2014} Whenever a land you control enters, you gain 1 life.";
        let (_, annotations) = parse_permanent(text, "");
        assert_eq!(annotations.len(), 2);
        assert_eq!(annotations[0].kind, AnnotationKind::AbilityWord);
        // label = "Landfall —" (em-dash is 3 bytes)
        let em_dash = '\u{2014}';
        let label = format!("Landfall {em_dash}");
        assert_eq!(annotations[0].start, 0);
        assert_eq!(annotations[0].end, label.len()); // 9 + 3 = 12
        assert_eq!(annotations[1].kind, AnnotationKind::Unparsed);
        let right = "Whenever a land you control enters, you gain 1 life.";
        let right_start = text.find(right).unwrap();
        assert_eq!(annotations[1].start, right_start);
        assert_eq!(annotations[1].end, text.len());
    }

    #[test]
    fn em_dash_cr702_keyword_emits_parsed_unimplemented_annotation() {
        let text = "Cumulative upkeep\u{2014}Add {R}.";
        let (_, annotations) = parse_permanent(text, "");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].kind, AnnotationKind::ParsedUnimplemented);
        assert_eq!(annotations[0].start, 0);
        assert_eq!(annotations[0].end, text.len());
    }

    #[test]
    fn activated_with_unknown_effect_emits_parsed_unimplemented_annotation() {
        let text = "{T}: Create a 1/1 token.";
        let (_, annotations) = parse_permanent(text, "");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].kind, AnnotationKind::ParsedUnimplemented);
        assert_eq!(annotations[0].start, 0);
        assert_eq!(annotations[0].end, text.len());
    }

    #[test]
    fn etb_with_unknown_effect_emits_parsed_unimplemented_annotation() {
        let text = "When this enters, create a 1/1 token.";
        let (_, annotations) = parse_permanent(text, "");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].kind, AnnotationKind::ParsedUnimplemented);
        assert_eq!(annotations[0].start, 0);
        assert_eq!(annotations[0].end, text.len());
    }

    #[test]
    fn fully_parsed_spans_emit_active_annotations() {
        let (_, a1) = parse_permanent("When this enters, draw a card.", "");
        assert_eq!(a1.len(), 1);
        assert_eq!(a1[0].kind, AnnotationKind::Active);
        let (_, a2) = parse_permanent("{T}: Add {G}.", "");
        assert_eq!(a2.len(), 1);
        assert_eq!(a2[0].kind, AnnotationKind::Active);
        let (_, a3) = parse_permanent("Flying", "");
        assert_eq!(a3.len(), 1);
        assert_eq!(a3[0].kind, AnnotationKind::Active);
    }

    // ── Task 2: Active annotation tests ──────────────────────────────────────

    #[test]
    fn flying_emits_active_annotation() {
        let (_, anns) = parse_permanent("Flying", "Test");
        assert!(
            anns.iter().any(|a| a.kind == AnnotationKind::Active),
            "expected Active annotation for implemented keyword"
        );
    }

    #[test]
    fn unimplemented_activation_cost_emits_parsed_unimplemented_annotation() {
        // "Sacrifice a creature" is an unimplemented cost component
        let (_, anns) = parse_permanent("Sacrifice a creature: Draw a card.", "Test");
        assert!(
            anns.iter()
                .any(|a| a.kind == AnnotationKind::ParsedUnimplemented),
            "expected ParsedUnimplemented annotation for unimplemented activation cost"
        );
    }

    #[test]
    fn clean_activated_ability_emits_active_annotation() {
        let (_, anns) = parse_permanent("{T}: Add {G}.", "Test");
        assert!(
            anns.iter().any(|a| a.kind == AnnotationKind::Active),
            "expected Active annotation for fully-parsed activated ability"
        );
    }

    #[test]
    fn etb_trigger_emits_active_annotation() {
        let (_, anns) = parse_permanent("When this enters, you gain 1 life.", "Test");
        assert!(
            anns.iter().any(|a| a.kind == AnnotationKind::Active),
            "expected Active annotation for parsed ETB trigger"
        );
    }

    #[test]
    fn continuous_pt_effect_emits_active_annotation() {
        let (_, anns) = parse_permanent("Creatures you control get +1/+1.", "Test");
        assert!(
            anns.iter().any(|a| a.kind == AnnotationKind::Active),
            "expected Active annotation for continuous PT effect"
        );
    }

    // ── Promoted keyword families (Task 3) ────────────────────────────────────

    #[test]
    fn fear_parses_as_static_ability() {
        let (spans, _) = parse_permanent("Fear", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Static(KeywordAbility::Fear))]
        );
    }

    #[test]
    fn intimidate_parses_as_static_ability() {
        let (spans, _) = parse_permanent("Intimidate", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Static(KeywordAbility::Intimidate))]
        );
    }

    #[test]
    fn battle_cry_parses_as_static_ability() {
        let (spans, _) = parse_permanent("Battle cry", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Static(KeywordAbility::BattleCry))]
        );
    }

    #[test]
    fn ward_mana_parses_as_triggered_ability() {
        // CR 702.21a: Ward is a triggered ability; emitted as TriggeredAbility { trigger: TargetedBy }.
        use crate::types::ability::{
            CostComponent, TriggerEvent, TriggerTargetMode, TriggeredAbility, TurnOwner,
        };
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip};
        let (spans, _) = parse_permanent("Ward {2}", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::TargetedBy {
                    controller: TurnOwner::Opponent,
                },
                condition: None,
                target_mode: TriggerTargetMode::None,
                effect: vec![EffectStep::Payment {
                    cost: vec![CostComponent::Mana(ManaCost {
                        pips: vec![ManaPip::Generic(2)]
                    })],
                    on_paid: vec![],
                    on_declined: vec![EffectStep::CounterSpell],
                }],
            }))]
        );
    }

    #[test]
    fn ward_life_parses_as_triggered_ability() {
        // CR 702.21a: Ward—Pay N life. is a triggered ability; emitted as TriggeredAbility { trigger: TargetedBy }.
        use crate::types::ability::{
            CostComponent, TriggerEvent, TriggerTargetMode, TriggeredAbility, TurnOwner,
        };
        use crate::types::effect::EffectStep;
        let (spans, _) = parse_permanent("Ward\u{2014}Pay 2 life.", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Triggered(TriggeredAbility {
                trigger: TriggerEvent::TargetedBy {
                    controller: TurnOwner::Opponent,
                },
                condition: None,
                target_mode: TriggerTargetMode::None,
                effect: vec![EffectStep::Payment {
                    cost: vec![CostComponent::PayLife(2)],
                    on_paid: vec![],
                    on_declined: vec![EffectStep::CounterSpell],
                }],
            }))]
        );
    }

    #[test]
    fn islandwalk_parses_as_landwalk() {
        use crate::types::LandwalkKind;
        let (spans, _) = parse_permanent("Islandwalk", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Static(KeywordAbility::Landwalk(
                LandwalkKind::LandType("Island".to_string())
            )))]
        );
    }

    #[test]
    fn swampwalk_parses_as_landwalk() {
        use crate::types::LandwalkKind;
        let (spans, _) = parse_permanent("Swampwalk", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Static(KeywordAbility::Landwalk(
                LandwalkKind::LandType("Swamp".to_string())
            )))]
        );
    }

    #[test]
    fn nonbasic_landwalk_parses_as_nonbasic() {
        use crate::types::LandwalkKind;
        let (spans, _) = parse_permanent("Nonbasic landwalk", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Static(KeywordAbility::Landwalk(
                LandwalkKind::Nonbasic
            )))]
        );
    }

    #[test]
    fn protection_from_blue_parses_as_protection() {
        use crate::types::mana::ManaColor;
        let (spans, _) = parse_permanent("Protection from blue", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Static(
                KeywordAbility::ProtectionFrom(crate::types::ability::ProtectionQuality::Color(
                    ManaColor::Blue
                ))
            ))]
        );
    }

    #[test]
    fn protection_from_artifacts_parses_as_protection() {
        let (spans, _) = parse_permanent("Protection from artifacts", "");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::ProtectionFrom(
                crate::types::ability::ProtectionQuality::CardType(
                    crate::types::card::CardType::Artifact
                )
            ))]
        );
    }

    // ── Conditional counter-spell parsing (Task 6) ────────────────────────────

    #[test]
    fn disdainful_stroke_parses_min_mana_value() {
        use crate::types::ability::{Rule, RulesText, TargetFilter};
        use crate::types::effect::EffectStep;
        let text = "Counter target spell with mana value 4 or greater.";
        let (spans, _) = parse_instant_or_sorcery(text, "Disdainful Stroke");
        let RulesText::Active(Rule::SpellAbility(sa)) = &spans[0] else {
            panic!()
        };
        assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
        let TargetFilter::Spell(f) = &sa.target_requirements[0] else {
            panic!()
        };
        assert_eq!(f.min_mana_value, Some(4));
        assert_eq!(f.max_mana_value, None);
        assert!(f.any_of_colors.is_empty());
    }

    #[test]
    fn max_mana_value_spell_parses_correctly() {
        use crate::types::ability::{Rule, RulesText, TargetFilter};
        use crate::types::effect::EffectStep;
        let text = "Counter target spell with mana value 3 or less.";
        let (spans, _) = parse_instant_or_sorcery(text, "Test");
        let RulesText::Active(Rule::SpellAbility(sa)) = &spans[0] else {
            panic!()
        };
        assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
        let TargetFilter::Spell(f) = &sa.target_requirements[0] else {
            panic!()
        };
        assert_eq!(f.max_mana_value, Some(3));
        assert_eq!(f.min_mana_value, None);
    }

    #[test]
    fn flashfreeze_parses_color_filter() {
        use crate::types::ability::{Rule, RulesText, TargetFilter};
        use crate::types::effect::EffectStep;
        use crate::types::mana::ManaColor;
        let text = "Counter target red or green spell.";
        let (spans, _) = parse_instant_or_sorcery(text, "Flashfreeze");
        let RulesText::Active(Rule::SpellAbility(sa)) = &spans[0] else {
            panic!()
        };
        assert_eq!(sa.steps, vec![EffectStep::CounterSpell]);
        let TargetFilter::Spell(f) = &sa.target_requirements[0] else {
            panic!()
        };
        assert!(f.any_of_colors.contains(&ManaColor::Red));
        assert!(f.any_of_colors.contains(&ManaColor::Green));
    }

    #[test]
    fn single_color_filter_parses_correctly() {
        use crate::types::ability::{Rule, RulesText, TargetFilter};
        use crate::types::mana::ManaColor;
        let text = "Counter target blue spell.";
        let (spans, _) = parse_instant_or_sorcery(text, "Test");
        let RulesText::Active(Rule::SpellAbility(sa)) = &spans[0] else {
            panic!()
        };
        let TargetFilter::Spell(f) = &sa.target_requirements[0] else {
            panic!()
        };
        assert_eq!(f.any_of_colors, vec![ManaColor::Blue]);
    }

    // ── Conditional counter-spell parsing (Task 7) ────────────────────────────

    #[test]
    fn mana_leak_parses_unless_mana() {
        use crate::types::ability::{CostComponent, Rule, RulesText};
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip};
        let text = "Counter target spell unless its controller pays {3}.";
        let (spans, _) = parse_instant_or_sorcery(text, "Mana Leak");
        let RulesText::Active(Rule::SpellAbility(sa)) = &spans[0] else {
            panic!()
        };
        assert_eq!(sa.steps.len(), 1);
        let EffectStep::Payment {
            cost,
            on_paid,
            on_declined,
        } = &sa.steps[0]
        else {
            panic!()
        };
        assert_eq!(
            cost,
            &vec![CostComponent::Mana(ManaCost {
                pips: vec![ManaPip::Generic(3)]
            })]
        );
        assert!(on_paid.is_empty());
        assert_eq!(on_declined, &vec![EffectStep::CounterSpell]);
    }

    #[test]
    fn quench_parses_unless_two_mana() {
        use crate::types::ability::{Rule, RulesText};
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip};
        let text = "Counter target spell unless its controller pays {2}.";
        let (spans, _) = parse_instant_or_sorcery(text, "Quench");
        let RulesText::Active(Rule::SpellAbility(sa)) = &spans[0] else {
            panic!()
        };
        let EffectStep::Payment { cost, .. } = &sa.steps[0] else {
            panic!()
        };
        assert_eq!(
            cost,
            &vec![crate::types::ability::CostComponent::Mana(ManaCost {
                pips: vec![ManaPip::Generic(2)]
            })]
        );
    }

    #[test]
    fn life_payment_counter_parses_unless_life() {
        use crate::types::ability::{CostComponent, Rule, RulesText};
        use crate::types::effect::EffectStep;
        let text = "Counter target spell unless its controller pays 3 life.";
        let (spans, _) = parse_instant_or_sorcery(text, "Test");
        let RulesText::Active(Rule::SpellAbility(sa)) = &spans[0] else {
            panic!()
        };
        let EffectStep::Payment { cost, .. } = &sa.steps[0] else {
            panic!()
        };
        assert_eq!(cost, &vec![CostComponent::PayLife(3)]);
    }

    #[test]
    fn condescend_parses_unless_x_with_trailing_scry() {
        use crate::types::ability::{AnnotationKind, CostComponent, Rule, RulesText};
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip};
        let text = "Counter target spell unless its controller pays {X}. Scry 2. \
            (Look at the top two cards of your library, then put any number of them \
            on the bottom and the rest on top in any order.)";
        let (spans, annotations) = parse_instant_or_sorcery(text, "Condescend");
        let RulesText::Active(Rule::SpellAbility(sa)) = &spans[0] else {
            panic!()
        };
        assert_eq!(sa.steps.len(), 2);
        let EffectStep::Payment {
            cost, on_declined, ..
        } = &sa.steps[0]
        else {
            panic!()
        };
        assert_eq!(
            cost,
            &vec![CostComponent::Mana(ManaCost {
                pips: vec![ManaPip::X]
            })]
        );
        assert_eq!(on_declined, &vec![EffectStep::CounterSpell]);
        assert!(matches!(sa.steps[1], EffectStep::Unimplemented(_)));

        assert!(
            annotations
                .iter()
                .any(|a| a.kind == AnnotationKind::ParsedUnimplemented
                    && &text[a.start..a.end] == "Scry 2")
        );
        assert!(
            annotations
                .iter()
                .any(|a| a.kind == AnnotationKind::ReminderText
                    && text[a.start..a.end].starts_with("(Look at the top two cards"))
        );
    }

    #[test]
    fn glorious_anthem_parses_as_continuous_effect() {
        use super::parse_permanent;
        use crate::types::{ControllerFilter, PTDelta, Rule, RulesText, card::CardType};

        let (spans, _) = parse_permanent("Creatures you control get +1/+1.", "Glorious Anthem");
        assert_eq!(spans.len(), 1);
        match &spans[0] {
            RulesText::Active(Rule::Continuous(effect)) => {
                assert!(matches!(
                    effect.subject_filter.controller,
                    ControllerFilter::You
                ));
                assert_eq!(effect.subject_filter.card_types, vec![CardType::Creature]);
                assert_eq!(
                    effect.pt_modification,
                    Some(PTDelta {
                        power: 1,
                        toughness: 1
                    })
                );
            }
            other => panic!("expected Rule::Continuous, got {other:?}"),
        }
    }

    #[test]
    fn color_anthem_parses_as_continuous_effect() {
        use super::parse_permanent;
        use crate::types::{ControllerFilter, PTDelta, Rule, RulesText, mana::ManaColor};

        let (spans, _) = parse_permanent("White creatures get +1/+1.", "Crusade");
        assert_eq!(spans.len(), 1);
        match &spans[0] {
            RulesText::Active(Rule::Continuous(effect)) => {
                assert!(matches!(
                    effect.subject_filter.controller,
                    ControllerFilter::Any
                ));
                assert_eq!(effect.subject_filter.colors, vec![ManaColor::White]);
                assert_eq!(
                    effect.pt_modification,
                    Some(PTDelta {
                        power: 1,
                        toughness: 1
                    })
                );
            }
            other => panic!("expected Rule::Continuous, got {other:?}"),
        }
    }

    #[test]
    fn subtype_anthem_parses_as_continuous_effect() {
        use super::parse_permanent;
        use crate::types::{Rule, RulesText};

        let (spans, _) = parse_permanent(
            "Creatures you control with Elf get +1/+1.",
            "Elvish Archdruid",
        );
        assert_eq!(spans.len(), 1);
        match &spans[0] {
            RulesText::Active(Rule::Continuous(effect)) => {
                assert_eq!(effect.subject_filter.subtypes, vec!["Elf".to_string()]);
            }
            other => panic!("expected Rule::Continuous, got {other:?}"),
        }
    }

    // ── parse_protection_quality / protection from / hexproof from ─────────────

    #[test]
    fn protection_from_everything_parses() {
        let (spans, _) = parse_permanent("Protection from everything", "Test");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::ProtectionFrom(
                crate::types::ability::ProtectionQuality::Everything
            ))]
        );
    }

    #[test]
    fn protection_from_artifacts_parses() {
        let (spans, _) = parse_permanent("Protection from artifacts", "Test");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::ProtectionFrom(
                crate::types::ability::ProtectionQuality::CardType(
                    crate::types::card::CardType::Artifact
                )
            ))]
        );
    }

    #[test]
    fn protection_from_vampire_creatures_parses() {
        let (spans, _) = parse_permanent("Protection from vampire creatures", "Test");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::ProtectionFrom(
                crate::types::ability::ProtectionQuality::CreatureType("Vampire".into())
            ))]
        );
    }

    #[test]
    fn hexproof_from_black_parses() {
        let (spans, _) = parse_permanent("Hexproof from black", "Test");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::HexproofFrom(
                crate::types::ability::ProtectionQuality::Color(ManaColor::Black)
            ))]
        );
    }

    #[test]
    fn hexproof_from_artifacts_parses() {
        let (spans, _) = parse_permanent("Hexproof from artifacts", "Test");
        assert_eq!(
            spans,
            vec![active(KeywordAbility::HexproofFrom(
                crate::types::ability::ProtectionQuality::CardType(
                    crate::types::card::CardType::Artifact
                )
            ))]
        );
    }

    #[test]
    fn parse_kicker_mana_cost() {
        use crate::types::mana::{ManaCost, ManaPip};
        let (spans, _) = parse_permanent("Kicker {1}{U}", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Kicker {
                additional_cost: ManaCost {
                    pips: vec![ManaPip::Generic(1), ManaPip::Blue]
                }
            })]
        );
    }

    #[test]
    fn parse_multikicker_mana_cost() {
        use crate::types::mana::{ManaCost, ManaPip};
        let (spans, _) = parse_permanent("Multikicker {G}", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Multikicker {
                additional_cost: ManaCost {
                    pips: vec![ManaPip::Green]
                }
            })]
        );
    }

    #[test]
    fn parse_dash_mana_cost() {
        use crate::types::mana::{ManaCost, ManaPip};
        let (spans, _) = parse_permanent("Dash {R}", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Dash {
                alternative_cost: ManaCost {
                    pips: vec![ManaPip::Red]
                }
            })]
        );
    }

    #[test]
    fn parse_evoke_mana_cost() {
        use crate::types::mana::{ManaCost, ManaPip};
        let (spans, _) = parse_permanent("Evoke {2}{U}", "Test");
        assert_eq!(
            spans,
            vec![RulesText::Active(Rule::Evoke {
                alternative_cost: ManaCost {
                    pips: vec![ManaPip::Generic(2), ManaPip::Blue]
                }
            })]
        );
    }

    #[test]
    fn parse_kicker_malformed_cost_falls_back_to_unimplemented() {
        let (spans, _) = parse_permanent("Kicker badcost", "Test");
        assert_eq!(
            spans,
            vec![RulesText::ParsedUnimplemented("Kicker badcost".to_string())]
        );
    }
}
