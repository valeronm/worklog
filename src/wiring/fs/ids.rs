use crate::domain::id::DocumentId;
use crate::domain::ports::{Ids, StoreError};

pub struct RandomIds;

impl Ids for RandomIds {
    fn mint(&self) -> Result<DocumentId, StoreError> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| StoreError::io("the system's random source", e))?;
        Ok(DocumentId::from_bytes(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_minted_ids_differ_and_parse() {
        let (one, other) = (RandomIds.mint().unwrap(), RandomIds.mint().unwrap());
        assert_ne!(one, other);
        assert_eq!(DocumentId::parse(one.as_str()), Ok(one));
    }
}
