# UI Thin-Client Overhaul

**Date:** 2026-06-12
**Status:** Approved, pending implementation

## Goal

Make the web UI layer as thin as possible. The JS should be a dumb renderer and dispatcher — no action logic, no validation. The Rust engine generates the list of available actions per card; the JS renders buttons and fires them.

## Scope

1. Separate CSS and JS out of `serve.html` into standalone source files.
2. Replace JS action-building functions with a Rust-generated `actions` field on each `CardView`.
3. Remove all JS validation; trust the engine completely.
4. Right-click always opens the action popup (even with 0 actions). Left-click auto-dispatches if exactly one payable action, otherwise opens popup.
5. Inject intrinsic land abilities into the engine (not serve.rs) so basic lands are treated the same as cards with parsed activated abilities.

## Section 1: File structure

`serve.html` becomes an HTML skeleton (~40 lines) with `<link>` and `<script>` tags. All CSS moves to `src/serve.css`; all JS moves to `src/serve.js`. Both are embedded at compile time via `include_str!` and served on dedicated Axum routes:

- `GET /static/app.css` → `include_str!("serve.css")`
- `GET /static/app.js`  → `include_str!("serve.js")`

No runtime file I/O. Consistent with the existing `serve.html` embedding pattern.

## Section 2: Data model

### `CardView` — removed fields

| Field | Reason |
|---|---|
| `can_cast: bool` | Replaced by `actions` |
| `can_attack: bool` | Replaced by `actions` |
| `can_block: bool` | Replaced by `actions` |
| `can_cycle: bool` | Replaced by `actions` |
| `cycling_cost: Option<String>` | Used only for action building; now a Rust concern |
| `activated_abilities: Vec<ActivatedAbilityView>` | Used only for action building; now a Rust concern |
| `valid_targets: Vec<TargetView>` | Used only for action building; now a Rust concern |

### `CardView` — added field

```rust
actions: Vec<ActionItemView>
```

### Removed types

`ActivatedAbilityView` and `TargetView` are no longer part of the card view and are removed from `serve.rs`.

### New types

```rust
struct ActionItemView {
    label: String,
    can_pay_cost: bool,
    #[serde(flatten)]
    kind: ActionItemKind,
}

#[serde(tag = "kind", rename_all = "snake_case")]
enum ActionItemKind {
    // Pre-built JSON payload posted verbatim to /action
    Server { action: serde_json::Value },
    // Toggle creature in/out of client-side attacker-staging list
    ToggleAttacker { object_id: u64 },
    // Assign creature as blocker for the given attacker (client-side staging)
    AssignBlocker { blocker_id: u64, attacker_id: u64 },
}
```

`can_pay_cost: true` means the action is fully executable now. `can_pay_cost: false` means the action is structurally valid (correct step, correct player, etc.) but the player cannot currently afford the costs. Actions that fail for any reason other than cost are omitted entirely from the list.

`ToggleAttacker` and `AssignBlocker` have no mana cost and always carry `can_pay_cost: true`.

The `Server` action payload is a `serde_json::Value` built with `serde_json::json!()` in `build_player_view`. It is posted verbatim by the JS to `/action` and must match the shape expected by `ActionRequest`.

## Section 3: Action computation

Actions are computed per card in `build_player_view`. The conceptual flow is: determine structural validity first (timing, priority, correct step, correct player), then evaluate cost payment. Structurally invalid → omit. Structurally valid but unaffordable → include with `can_pay_cost: false`. Both checks can be interleaved in practice.

### Hand — land
- Active player, sorcery-speed step (`PreCombatMain`/`PostCombatMain`), land drop available → `Server { play_land }`, `can_pay_cost: true`
- Otherwise → omit

### Hand — non-land, no targets
- Wrong step/player → omit
- Correct step/player, mana sufficient → `Server { cast_spell }`, `can_pay_cost: true`
- Correct step/player, mana insufficient → `Server { cast_spell }`, `can_pay_cost: false`

### Hand — non-land, with targets
- Same step/player/mana logic as above, but one action per legal target:
  `Server { cast_spell, object_id, targets: [t] }` — label `"Cast {name} → {target_name}"`
- If there are no legal targets (spell requires a target but none are available), the action is omitted entirely (structural check fails — the spell cannot legally be cast)

### Hand — cycling
- No priority → omit
- Has priority, mana sufficient → `Server { cycle_card }`, `can_pay_cost: true`
- Has priority, mana insufficient → `Server { cycle_card }`, `can_pay_cost: false`

### Battlefield land (after intrinsic-ability engine change — see Section 5)
- One `Server { activate_ability, index }` per activated ability (parsed or injected intrinsic)
- `can_pay_cost: false` if costs unaffordable (e.g. already tapped, insufficient mana)

### Battlefield creature — DeclareAttackers step
- `perm.can_attack()` and active player → `ToggleAttacker { object_id }`, `can_pay_cost: true`

### Battlefield creature — DeclareBlockers step
- `perm.can_block()` and defending player → one `AssignBlocker { blocker_id, attacker_id }` per creature in `state.combat.attackers`; `can_pay_cost: true`

### Battlefield creature — any step
- Per activated ability: `Server { activate_ability }`, same mana-only `can_pay_cost` logic

## Section 4: JS changes

### Removed functions
`getBattlefieldCreatureActions`, `getBattlefieldLandActions`, `getHandActions`, `isCardActionable`.

### New `findCard` helper
```js
function findCard(cardId, pid) {
    const p = pid === 0 ? currentState.p1 : currentState.p2;
    return p.hand.find(c => c.id === cardId)
        || p.lands.find(c => c.id === cardId)
        || p.creatures.find(c => c.id === cardId);
}
```

### New `dispatchAction(item)`
The only place in JS that knows about action kinds:
```js
function dispatchAction(item) {
    if (item.kind === 'server') {
        sendAction(item.action);
    } else if (item.kind === 'toggle_attacker') {
        const idx = attackersSelected.indexOf(item.object_id);
        if (idx >= 0) attackersSelected.splice(idx, 1);
        else attackersSelected.push(item.object_id);
        render(currentState);
    } else if (item.kind === 'assign_blocker') {
        if (blockersAssignment[item.blocker_id] === item.attacker_id)
            delete blockersAssignment[item.blocker_id];
        else
            blockersAssignment[item.blocker_id] = item.attacker_id;
        render(currentState);
    }
}
```

### New `buildPopupItems(actions)`
```js
function buildPopupItems(actions) {
    return actions.map(a => ({
        label: a.label,
        disabled: !a.can_pay_cost,
        onClick: a.can_pay_cost ? () => dispatchAction(a) : () => {},
    }));
}
```

### Combined `handleCardClick(cardId, pid, event, autoDispatchIfSingle)`
Left-click passes `true`, right-click passes `false`:
```js
function handleCardClick(cardId, pid, event, autoDispatchIfSingle) {
    if (!autoDispatchIfSingle) event.preventDefault();
    if (!currentState) return;
    closePopup();
    const card = pid >= 0 ? findCard(cardId, pid) : null;
    const actions = card ? card.actions : [];

    if (autoDispatchIfSingle) {
        if (actions.length === 1 && actions[0].can_pay_cost) {
            dispatchAction(actions[0]); return;
        }
        if (actions.length === 0) return;
    }

    const items = actions.length > 0
        ? buildPopupItems(actions)
        : [{ label: 'No valid actions', onClick: () => {} }];
    openPopup(items, event.target, 'Actions');
}
```

Wired in `cardHTML`:
```
onclick="handleCardClick(${card.id}, ${pid}, event, true)"
oncontextmenu="handleCardClick(${card.id}, ${pid}, event, false)"
```

### Visual state in `cardHTML`
`isCardActionable` is replaced by:
```js
const isActionable = card.actions && card.actions.some(a => a.can_pay_cost);
```

Tapped lands (all actions have `can_pay_cost: false`) correctly dim.

### CSS addition
```css
.popup-item.disabled { color: #555; border-color: #2a3a4a; cursor: default; }
.popup-item.disabled:hover { background: #1c2a3a; border-color: #2a4a6a; }
```

`openPopup` updated to apply `disabled` class when `item.disabled` is true.

## Section 5: Intrinsic land abilities (engine change)

Basic land types (Forest, Island, Mountain, Plains, Swamp) have intrinsic mana abilities per CR 305.6. Currently these are handled by a `tap_land` code path in the UI layer as a workaround. The correct model is to inject the corresponding `ActivatedAbility` into the card's abilities list at construction time.

**Change:** In `card_object.rs`, after building the `CardObject`, detect basic land subtypes and inject the appropriate `OracleSpan::Parsed(Ability::Activated(...))`:

| Subtype | Injected ability |
|---|---|
| Forest | `{T}: Add {G}` |
| Island | `{T}: Add {U}` |
| Mountain | `{T}: Add {R}` |
| Plains | `{T}: Add {W}` |
| Swamp | `{T}: Add {B}` |

Once injected, `activate_ability` handles them identically to parsed abilities. The `tap_land` action type in `ActionRequest` and `tap_land_for_mana` engine function are kept for now (the mana-checkpoint / reset-mana machinery still uses them) but are no longer generated as UI actions.

This is an estimated ~30-line addition to `card_object.rs`.

## Section 6: Test rewrites

`can_cast`, `can_attack`, `can_block` assertions are replaced with action-based checks. Helper functions:

```rust
fn has_server_action(card: &CardView) -> bool {
    card.actions.iter().any(|a| matches!(a.kind, ActionItemKind::Server { .. }))
}
fn has_payable_server_action(card: &CardView) -> bool {
    card.actions.iter().any(|a| a.can_pay_cost && matches!(a.kind, ActionItemKind::Server { .. }))
}
fn has_toggle_attacker(card: &CardView) -> bool {
    card.actions.iter().any(|a| matches!(a.kind, ActionItemKind::ToggleAttacker { .. }))
}
fn has_assign_blocker(card: &CardView) -> bool {
    card.actions.iter().any(|a| matches!(a.kind, ActionItemKind::AssignBlocker { .. }))
}
```

| Old | New |
|---|---|
| `assert!(card.can_cast)` | `assert!(has_payable_server_action(card))` |
| `assert!(!card.can_cast)` | `assert!(card.actions.is_empty())` |
| `assert!(p1_c.can_attack)` | `assert!(has_toggle_attacker(p1_c))` |
| `assert!(!p2_c.can_attack)` | `assert!(!has_toggle_attacker(p2_c))` |
| `assert!(p2_c.can_block)` | `assert!(has_assign_blocker(p2_c))` |
| `assert!(!p1_c.can_block)` | `assert!(!has_assign_blocker(p1_c))` |
