//! Turning one trigger of a relation off and on.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::{quoted_iden, quoted_table},
};
use sea_query::{DynIden, IntoIden, TableName};

/// Whether a trigger fires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerState {
    /// The trigger fires.
    Enabled,
    /// The trigger does not fire.
    Disabled,
}

/// `ALTER TABLE <relation> {ENABLE | DISABLE} TRIGGER <trigger>`.
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerSwitch {
    /// The relation the trigger belongs to.
    relation: TableName,
    /// The trigger switched.
    trigger: DynIden,
    /// Whether it fires afterwards.
    state: TriggerState,
}

impl TriggerSwitch {
    /// Put `trigger` of `relation` in `state`.
    pub fn new(relation: impl Into<TableName>, trigger: impl IntoIden, state: TriggerState) -> TriggerSwitch {
        TriggerSwitch {
            relation: relation.into(),
            trigger: trigger.into_iden(),
            state,
        }
    }
}

impl RenderedCommand for TriggerSwitch {
    fn rendered(&self) -> String {
        let state = match self.state {
            TriggerState::Enabled => "ENABLE",
            TriggerState::Disabled => "DISABLE",
        };
        format!("ALTER TABLE {} {state} TRIGGER {}", quoted_table(&self.relation), quoted_iden(&self.trigger))
    }
}

impl Command for TriggerSwitch {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        trigger_switch::{TriggerState, TriggerSwitch},
    };
    use sea_query::{Alias, TableName};

    #[test]
    fn a_trigger_is_switched_by_name_on_its_relation() {
        pretty_assertions::assert_eq!(
            TriggerSwitch::new(
                TableName::from(Alias::new("attribute_names")),
                Alias::new("trg_append_only"),
                TriggerState::Disabled
            )
            .rendered(),
            r#"ALTER TABLE "attribute_names" DISABLE TRIGGER "trg_append_only""#
        );
    }
}
