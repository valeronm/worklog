use std::rc::Rc;

use crate::draft::is_drafted;
use crate::lookup::{Lookup, refuse_ended};
use crate::save::{Admitted, stepped};
use crate::{Deps, Failure, Written};
use worklog_domain::id::DocumentId;
use worklog_domain::version::{Fields, Version};
use worklog_schema::{Record, step};

pub(super) struct Open {
    pub id: DocumentId,
    pub label: String,
    pub head: Rc<Version>,
    pub record: Record,
}

pub(super) fn open(deps: &Deps, lookup: &Lookup, address: &str) -> Result<Open, Failure> {
    let id = lookup.one(address)?;
    let label = lookup.label(&id)?;
    let drafted = is_drafted(&deps.read(), &id)?;
    let (head, record) = lookup.writable(&id, drafted, &label)?;
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
