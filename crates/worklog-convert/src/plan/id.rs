use worklog::domain::id::DocumentId;

use crate::old::Kind;

fn hashed(domain: &str, text: &str) -> DocumentId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"worklog-convert\0");
    hasher.update(domain.as_bytes());
    hasher.update(b"\0");
    hasher.update(text.as_bytes());
    let hash = hasher.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash.as_bytes()[..16]);
    DocumentId::from_bytes(bytes)
}

/// The id of the document whose rename chain starts at `origin`, a slug of the store being
/// converted; the same kind and slug give the same id in every run.
pub(crate) fn document_id(kind: Kind, origin: &str) -> DocumentId {
    hashed("document", &format!("{}\0{origin}", kind.word()))
}

/// The id of a document the conversion makes from no document of the store, by a description
/// that names it among all such documents.
pub(crate) fn generated_id(description: &str) -> DocumentId {
    hashed("generated", description)
}

/// The id of the topic a `[topics.<name>]` table creates, from the name as
/// `Decisions::topics` lists it.
pub(crate) fn created_topic_id(name: &str) -> DocumentId {
    generated_id(&format!("topic\0{name}"))
}
