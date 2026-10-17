use super::Admitted;
use crate::app::{Failure, Stored, Written};
use crate::domain::ports::Store;

#[derive(Clone, Copy)]
pub(in crate::app) struct Writer<'a>(&'a dyn Store);

impl<'a> Writer<'a> {
    pub(in crate::app) fn new(store: &'a dyn Store) -> Writer<'a> {
        Writer(store)
    }

    pub(in crate::app) fn read(&self) -> Stored<'a> {
        Stored::new(self.0)
    }

    pub(super) fn store(&self, admitted: Admitted) -> Result<Written, Failure> {
        self.0.put(&admitted.version)?;
        Ok(Written {
            document: admitted.version.envelope.document,
            label: admitted.label,
            version: admitted.version.id,
        })
    }
}
