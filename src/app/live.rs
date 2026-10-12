use crate::app::draft::refuse_open;
use crate::app::lookup::{Lookup, refuse_ended};
use crate::app::save::{Admitted, stepped};
use crate::app::{Deps, Failure, Written};
use crate::domain::id::DocumentId;
use crate::domain::schema::{Record, step};
use crate::domain::version::{Fields, Version};

pub(super) struct Open {
    pub id: DocumentId,
    pub label: String,
    pub head: Version,
    pub record: Record,
}

pub(super) fn open(deps: &Deps, lookup: &Lookup, address: &str) -> Result<Open, Failure> {
    let id = lookup.one(address)?;
    let label = lookup.label(&id)?;
    refuse_open(deps, &id, &label)?;
    let (head, record) = lookup.writable(&id, &label)?;
    Ok(Open {
        id,
        label,
        head,
        record,
    })
}

impl Open {
    pub(super) fn unended(&self) -> Result<(), Failure> {
        refuse_ended(&self.record, &self.label)
    }

    pub(super) fn shown(&self) -> Fields {
        step::shown(&self.head.fields)
    }

    pub(super) fn checked(
        &self,
        deps: &Deps,
        lookup: &Lookup,
        fields: &Fields,
        label: &str,
    ) -> Result<Admitted, Failure> {
        stepped(
            deps,
            lookup,
            &self.id,
            &self.head.envelope.kind,
            fields,
            &self.head.body,
            label,
        )
    }

    pub(super) fn store(
        &self,
        deps: &Deps,
        lookup: &Lookup,
        fields: &Fields,
        label: &str,
    ) -> Result<Written, Failure> {
        self.checked(deps, lookup, fields, label)?.store(deps)
    }
}
