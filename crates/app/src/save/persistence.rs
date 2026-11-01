use super::Admitted;
use crate::{Failure, Stored, Written};
use worklog_domain::ports::Store;

#[derive(Clone, Copy)]
pub(crate) struct Writer<'a>(&'a dyn Store);

impl<'a> Writer<'a> {
    pub(crate) fn new(store: &'a dyn Store) -> Writer<'a> {
        Writer(store)
    }

    pub(crate) fn read(&self) -> Stored<'a> {
        Stored::new(self.0)
    }

    pub(super) fn store(&self, admitted: Admitted) -> Result<Written, Failure> {
        self.0.put(&admitted.version)?;
        let header = admitted.version.into_header();
        Ok(Written {
            document: header.envelope.document,
            label: admitted.label,
            version: header.id,
        })
    }
}
