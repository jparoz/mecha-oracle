use crate::types::OracleSpan::ParsedUnimplemented;
use crate::types::ability::{ActivationCost, CostComponent};
use crate::types::effect::{Effect, EffectStep};
use crate::types::mana::{ManaColor, ManaCost, ManaPip, ManaPool};
use crate::types::{
    AbilityAST, IgnoredKind, OracleSpan,
    ability::{ActivatedAbility, StaticAbility},
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

fn parse_activation_cost(s: &str) -> ActivationCost {
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
    None
}

fn parse_ability_effect(s: &str) -> Option<Effect> {
    let s = s.trim_end_matches('.');
    s.split(". ")
        .filter(|step| !step.is_empty())
        .map(|step| try_parse_effect_step(step.trim()))
        .collect()
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
    let s = kw.to_lowercase();
    let s = s.as_str();

    // ── Fully-implemented keywords ────────────────────────────────────────────
    macro_rules! parsed {
        ($variant:ident) => {
            OracleSpan::Parsed(AbilityAST::Static(StaticAbility::$variant))
        };
    }
    match s {
        "flying" => return parsed!(Flying),
        "reach" => return parsed!(Reach),
        "trample" => return parsed!(Trample),
        "first strike" => return parsed!(FirstStrike),
        "double strike" => return parsed!(DoubleStrike),
        "vigilance" => return parsed!(Vigilance),
        "haste" => return parsed!(Haste),
        "lifelink" => return parsed!(Lifelink),
        "deathtouch" => return parsed!(Deathtouch),
        "menace" => return parsed!(Menace),
        "indestructible" => return parsed!(Indestructible),
        "defender" => return parsed!(Defender),
        "shadow" => return parsed!(Shadow),
        "horsemanship" => return parsed!(Horsemanship),
        "skulk" => return parsed!(Skulk),
        "decayed" => return parsed!(Decayed),
        _ => {}
    }

    // ── CR 702 recognised-but-unimplemented keywords ──────────────────────────
    if is_cr702_keyword(s) {
        return ParsedUnimplemented(kw.to_string());
    }

    OracleSpan::Unparsed(kw.to_string())
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
        // 702.8
        "flash" |
        // 702.11
        "hexproof" |
        // 702.13
        "intimidate" |
        // 702.18
        "shroud" |
        // 702.22
        "banding" |
        // 702.25
        "flanking" |
        // 702.26
        "phasing" |
        // 702.36
        "fear" |
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
        // 702.80
        "wither" |
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
        // 702.90
        "infect" |
        // 702.91
        "battle cry" |
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
        // 702.100
        "evolve" |
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
        // 702.149
        "training" |
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
    // 702.11 Hexproof from [quality]
    s.starts_with("hexproof from ") ||
    // 702.16 Protection from [quality]
    s.starts_with("protection from") ||
    // 702.21 Ward [cost] (may also use em-dash: Ward—Pay N life.)
    s.starts_with("ward") ||
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
    // 702.164 Toxic N
    s.starts_with("toxic ") ||
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
    // 702.14 Landwalk: islandwalk, swampwalk, nonbasic landwalk, etc.
    // kw_part used so "islandwalk" and "nonbasic landwalk" both match.
    kw_part.ends_with("walk") ||
    // 702.29 Typecycling: mountaincycling {2}, basic landcycling {2}, etc.
    // Split on '{' so cost is stripped before checking the suffix.
    kw_part.ends_with("cycling") ||
    // 702.48 [Type] offering: goblin offering, elf offering, etc.
    s.ends_with(" offering") ||
    // 702.22 Bands with other [type]
    s.starts_with("bands with other")
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

        // Em-dash at depth 0 → ability/flavour word line, or keyword with em-dash cost.
        if let Some(dash_pos) = find_at_depth_zero(paragraph, EM_DASH) {
            let left = paragraph[..dash_pos].trim();
            let right = paragraph[dash_pos + EM_DASH.len_utf8()..].trim();

            match match_keyword(left) {
                OracleSpan::ParsedUnimplemented(_) => {
                    // CR 702 keyword with em-dash syntax (Cumulative upkeep, Suspend, etc.).
                    // Emit the whole paragraph as a single recognised-unimplemented span.
                    spans.push(ParsedUnimplemented(paragraph.to_string()));
                    continue;
                }
                OracleSpan::Parsed(_) => {
                    // Fully-implemented keyword with em-dash — fall through to comma splitting.
                }
                _ => {
                    // Ability word or flavour word — emit label + right side.
                    let label = paragraph[..dash_pos + EM_DASH.len_utf8()].to_string();
                    spans.push(OracleSpan::Ignored(IgnoredKind::AbilityWord, label));
                    if !right.is_empty() {
                        spans.push(OracleSpan::Unparsed(right.to_string()));
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
                if let Some(effect) = parse_ability_effect(effect_str) {
                    spans.push(OracleSpan::Parsed(AbilityAST::Activated(
                        ActivatedAbility { cost, effect },
                    )));
                } else {
                    spans.push(OracleSpan::ParsedUnimplemented(paragraph.to_string()));
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
    fn unimplemented(text: &str) -> OracleSpan {
        OracleSpan::ParsedUnimplemented(text.to_string())
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
    fn cumulative_upkeep_emits_parsed_unimplemented() {
        let result = parse_oracle_text("Cumulative upkeep\u{2014}Add {R}.");
        assert_eq!(
            result,
            vec![unimplemented("Cumulative upkeep\u{2014}Add {R}.")]
        );
    }

    #[test]
    fn triggered_ability_becomes_unparsed() {
        assert_eq!(
            parse_oracle_text("When this creature enters, draw a card."),
            vec![
                unparsed("When this creature enters"),
                unparsed("draw a card."),
            ]
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
    fn all_implemented_keywords_parse() {
        let text = "Flying\nReach\nTrample\nFirst strike\nDouble strike\nVigilance\nHaste\nLifelink\nDeathtouch\nMenace\nIndestructible\nDefender\nShadow\nHorsemanship\nSkulk\nDecayed";
        let result = parse_oracle_text(text);
        assert_eq!(result.len(), 16);
        assert!(result.iter().all(|s| matches!(s, OracleSpan::Parsed(_))));
    }

    #[test]
    fn bare_unimplemented_keyword_emits_parsed_unimplemented() {
        assert_eq!(parse_oracle_text("Flash"), vec![unimplemented("Flash")]);
        assert_eq!(
            parse_oracle_text("Hexproof"),
            vec![unimplemented("Hexproof")]
        );
        assert_eq!(parse_oracle_text("Cascade"), vec![unimplemented("Cascade")]);
    }

    #[test]
    fn parameterised_keyword_emits_parsed_unimplemented() {
        assert_eq!(
            parse_oracle_text("Cycling {2}"),
            vec![unimplemented("Cycling {2}")]
        );
        assert_eq!(
            parse_oracle_text("Kicker {1}{U}"),
            vec![unimplemented("Kicker {1}{U}")]
        );
        assert_eq!(
            parse_oracle_text("Protection from black"),
            vec![unimplemented("Protection from black")]
        );
    }

    #[test]
    fn landwalk_variants_emit_parsed_unimplemented() {
        assert_eq!(
            parse_oracle_text("Islandwalk"),
            vec![unimplemented("Islandwalk")]
        );
        assert_eq!(
            parse_oracle_text("Nonbasic landwalk"),
            vec![unimplemented("Nonbasic landwalk")]
        );
    }

    #[test]
    fn typecycling_with_space_emits_parsed_unimplemented() {
        // 702.29e: "basic landcycling" has a space between the two type words
        assert_eq!(
            parse_oracle_text("Basic landcycling {2}"),
            vec![unimplemented("Basic landcycling {2}")]
        );
        assert_eq!(
            parse_oracle_text("Mountaincycling {1}"),
            vec![unimplemented("Mountaincycling {1}")]
        );
    }

    #[test]
    fn em_dash_keyword_emits_whole_paragraph_as_parsed_unimplemented() {
        // Suspend: "Suspend 2—{1}{U}"
        assert_eq!(
            parse_oracle_text("Suspend 2\u{2014}{1}{U}"),
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

    // ── Activated ability integration tests ──────────────────────────────────

    #[test]
    fn tap_add_green_parses_as_activated() {
        use crate::types::ability::{ActivatedAbility, CostComponent};
        use crate::types::effect::EffectStep;
        use crate::types::mana::ManaPool;
        let result = parse_oracle_text("{T}: Add {G}.");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility {
                cost,
                effect,
            })) if cost == &vec![CostComponent::Tap]
                && effect == &vec![EffectStep::AddMana(ManaPool { green: 1, ..Default::default() })]
        ));
    }

    #[test]
    fn two_tap_add_two_green_parses_as_activated() {
        use crate::types::ability::CostComponent;
        use crate::types::effect::EffectStep;
        use crate::types::mana::{ManaCost, ManaPip, ManaPool};
        let result = parse_oracle_text("{2}, {T}: Add {G}{G}.");
        assert_eq!(result.len(), 1);
        if let OracleSpan::Parsed(AbilityAST::Activated(ability)) = &result[0] {
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
        let result = parse_oracle_text("{1}: Draw a card.");
        assert_eq!(result.len(), 1);
        if let OracleSpan::Parsed(AbilityAST::Activated(ability)) = &result[0] {
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
        let result = parse_oracle_text("{T}: Mill 2.");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility { cost, effect }))
            if cost == &vec![CostComponent::Tap]
            && effect == &vec![EffectStep::Mill(2)]
        ));
    }

    #[test]
    fn reminder_text_colon_not_treated_as_activated() {
        // ({T}: Add {G}.) is reminder text — not an activated ability
        let result = parse_oracle_text("({T}: Add {G}.)");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            OracleSpan::Ignored(IgnoredKind::ReminderText, _)
        ));
    }

    #[test]
    fn sacrifice_cost_becomes_unimplemented_in_cost_activated_parsed() {
        use crate::types::ability::{ActivatedAbility, CostComponent};
        use crate::types::effect::EffectStep;
        use crate::types::mana::ManaPool;
        let result = parse_oracle_text("Sacrifice a creature: Add {G}{G}.");
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            OracleSpan::Parsed(AbilityAST::Activated(ActivatedAbility { cost, effect }))
            if cost == &vec![CostComponent::Unimplemented("Sacrifice a creature".to_string())]
            && effect == &vec![EffectStep::AddMana(ManaPool { green: 2, ..Default::default() })]
        ));
    }

    #[test]
    fn unknown_effect_becomes_parsed_unimplemented() {
        let result = parse_oracle_text("{T}: Create a 1/1 token.");
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], OracleSpan::ParsedUnimplemented(_)));
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
}
