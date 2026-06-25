//! Core data types for the rules engine.
//!
//! All engine functions consume and return `GameState`; everything else in this
//! module is the vocabulary those functions operate on.  The most important types
//! are:
//!
//! * [`GameState`] — the complete, immutable-snapshot game state
//! * [`CardObject`] — a single card instance (owns its [`CardDefinition`])
//! * [`PermanentState`] — battlefield-only data (tapped, damage, counters, …)
//! * [`StackObject`] / [`StackPayload`] — items on the stack
//! * [`RulesText`] / [`Rule`] — the parsed oracle-text representation
pub mod ability;
pub mod card;
pub mod card_object;
pub mod counter;
pub mod effect;
pub mod game_state;
pub mod ids;
pub mod mana;
pub mod permanent;
pub mod player;
pub mod stack;
pub mod step;
pub mod zone;

pub use ability::{
    ActivatedAbility, AnnotationKind, CardFilter, CastFilter, ContinuousEffect, ControllerFilter,
    Cost, CostComponent, DamageTargetKind, GameEvent, IgnoredKind, KeywordAbility, LandwalkKind,
    PermanentFilter, Rule, RulesText, SpellAbility, SpellFilter, TargetFilter, TextAnnotation,
    TriggerCondition, TriggerEvent, TriggerSubjectFilter, TriggerTargetMode, TriggeredAbility,
    TurnOwner,
};
pub use card::{CardDefinition, CardType, Supertype, TypeLine};
pub use card_object::CardObject;
pub use counter::CounterKind;
pub use effect::{Effect, EffectStep, EffectTarget};
pub use game_state::{CombatState, DelayedTrigger, GameState, ManaCheckpoint, PendingPayment};
pub use ids::{ObjectId, PlayerId};
pub use mana::{ManaColor, ManaCost, ManaPip, ManaPool, PaymentPlan};
pub use permanent::{PTDelta, PermanentState};
pub use player::Player;
pub use stack::{StackId, StackObject, StackPayload};
pub use step::{Phase, Step};
pub use zone::{Zone, ZoneOwner};
