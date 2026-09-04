use std::fmt;

use super::frontmatter::{self, FieldError, Fields, ParseError, Value};
use super::machine::MachineName;
use super::slug::{Kind, Slug};

/// The BLAKE3 hash of a version's bytes, which is also its file name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VersionId(String);

impl VersionId {
    #[must_use]
    pub fn of(text: &str) -> VersionId {
        VersionId(blake3::hash(text.as_bytes()).to_hex().to_string())
    }

    pub fn parse(text: &str) -> Result<VersionId, VersionError> {
        let ok = text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if ok {
            Ok(VersionId(text.to_owned()))
        } else {
            Err(VersionError::BadId(text.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `text` could be the start of an id: lowercase hex, and long
    /// enough not to match by accident.
    #[must_use]
    pub fn is_prefix(text: &str) -> bool {
        (6..=64).contains(&text.len())
            && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    /// Enough of the hash to tell versions of one document apart on a line.
    #[must_use]
    pub fn short(&self) -> &str {
        &self.0[..12]
    }
}

impl fmt::Display for VersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What made the version, so history and a fork report can name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    New,
    Save,
    Done,
    Drop,
    Recheck,
    Verify,
    Tombstone,
    Rename,
    Resolve,
    Claim,
    Unclaim,
    Migrate,
}

impl Operation {
    pub const ALL: [Operation; 12] = [
        Operation::New,
        Operation::Save,
        Operation::Done,
        Operation::Drop,
        Operation::Recheck,
        Operation::Verify,
        Operation::Tombstone,
        Operation::Rename,
        Operation::Resolve,
        Operation::Claim,
        Operation::Unclaim,
        Operation::Migrate,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::New => "new",
            Operation::Save => "save",
            Operation::Done => "done",
            Operation::Drop => "drop",
            Operation::Recheck => "recheck",
            Operation::Verify => "verify",
            Operation::Tombstone => "tombstone",
            Operation::Rename => "rename",
            Operation::Resolve => "resolve",
            Operation::Claim => "claim",
            Operation::Unclaim => "unclaim",
            Operation::Migrate => "migrate",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Operation> {
        Operation::ALL.into_iter().find(|op| op.as_str() == text)
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionBlock {
    pub parents: Vec<VersionId>,
    pub written: String,
    pub machine: MachineName,
    pub operation: Operation,
    /// The new slug, on the tombstone a rename leaves behind.
    pub superseded_by: Option<Slug>,
}

/// One immutable file of the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub id: VersionId,
    pub slug: Slug,
    pub block: VersionBlock,
    /// The kind's own fields, without `slug`, `kind` and `version`.
    pub fields: Fields,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionError {
    Frontmatter(ParseError),
    Field(FieldError),
    BadId(String),
    /// The bytes are not what the writer would emit for their content, so
    /// their hash would not survive a rewrite.
    NotCanonical,
    /// The file's name is not the hash of its bytes.
    IdMismatch {
        named: String,
        actual: String,
    },
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionError::Frontmatter(e) => e.fmt(f),
            VersionError::Field(e) => e.fmt(f),
            VersionError::BadId(id) => write!(f, "`{id}` is not a version id"),
            VersionError::NotCanonical => f.write_str("the bytes are not in the writer's shape"),
            VersionError::IdMismatch { named, actual } => {
                write!(f, "file named {named} hashes to {actual}")
            }
        }
    }
}

impl From<ParseError> for VersionError {
    fn from(e: ParseError) -> Self {
        VersionError::Frontmatter(e)
    }
}

impl From<FieldError> for VersionError {
    fn from(e: FieldError) -> Self {
        VersionError::Field(e)
    }
}

fn invalid(key: &str, reason: impl fmt::Display) -> VersionError {
    VersionError::Field(FieldError::invalid(key, reason))
}

/// The fields of a file as written: the document header, the kind's own
/// fields, and the `version` block last. Versions and drafts share this
/// shape, so a draft's bytes are a version's minus what `save` stamps.
#[must_use]
pub fn envelope(slug: &Slug, fields: &Fields, version: Value) -> Fields {
    let mut all = Fields::default();
    all.push_scalar("slug", slug.path());
    all.push_scalar("kind", slug.kind().dir());
    for (key, value) in fields.iter() {
        all.push(key, value.clone());
    }
    all.push("version", version);
    all
}

/// The inverse of `envelope`: the slug, the `version` value and the kind's
/// own fields.
pub fn open_envelope(mut fields: Fields) -> Result<(Slug, Value, Fields), VersionError> {
    let slug = fields.required("slug")?;
    let kind = fields.required("kind")?;
    let kind =
        Kind::from_dir(kind).ok_or_else(|| invalid("kind", format!("`{kind}` is no kind")))?;
    let slug = Slug::of_kind(kind, slug).map_err(|e| invalid("slug", e))?;
    let version = fields
        .remove("version")
        .ok_or(FieldError::Missing("version"))?;
    fields.remove("slug");
    fields.remove("kind");
    Ok((slug, version, fields))
}

/// The `parents` list of a version block.
pub fn parse_parents(block: &Fields) -> Result<Vec<VersionId>, VersionError> {
    block
        .list("parents")
        .ok_or(FieldError::Missing("version.parents"))?
        .iter()
        .map(|p| VersionId::parse(p))
        .collect()
}

impl VersionBlock {
    fn to_value(&self) -> Value {
        let mut entries = vec![
            (
                "parents".to_owned(),
                Value::List(self.parents.iter().map(|p| p.0.clone()).collect()),
            ),
            ("written".to_owned(), Value::Scalar(self.written.clone())),
            (
                "machine".to_owned(),
                Value::Scalar(self.machine.as_str().to_owned()),
            ),
            (
                "operation".to_owned(),
                Value::Scalar(self.operation.as_str().to_owned()),
            ),
        ];
        if let Some(slug) = &self.superseded_by {
            entries.push(("superseded_by".to_owned(), Value::Scalar(slug.to_string())));
        }
        Value::Map(entries)
    }

    fn from_entries(entries: &[(String, Value)]) -> Result<VersionBlock, VersionError> {
        let sub: Fields = entries.iter().cloned().collect();
        sub.reject_unknown(&[
            "parents",
            "written",
            "machine",
            "operation",
            "superseded_by",
        ])?;
        let parents = parse_parents(&sub)?;
        let written = sub
            .required("written")
            .map_err(|_| FieldError::Missing("version.written"))?;
        let machine = MachineName::parse(
            sub.required("machine")
                .map_err(|_| FieldError::Missing("version.machine"))?,
        )
        .map_err(|e| invalid("version.machine", e))?;
        let operation = sub
            .required("operation")
            .map_err(|_| FieldError::Missing("version.operation"))?;
        let operation = Operation::parse(operation).ok_or_else(|| {
            invalid(
                "version.operation",
                format!("`{operation}` is no operation"),
            )
        })?;
        let superseded_by = match sub.optional("superseded_by") {
            Some(text) => Some(Slug::parse(text).map_err(|e| invalid("version.superseded_by", e))?),
            None => None,
        };
        Ok(VersionBlock {
            parents,
            written: written.to_owned(),
            machine,
            operation,
            superseded_by,
        })
    }
}

impl Version {
    /// Builds the version from its parts; the id is the hash of the bytes
    /// `to_text` will produce.
    #[must_use]
    pub fn compose(slug: Slug, block: VersionBlock, fields: Fields, body: String) -> Version {
        let text = Version::render(&slug, &block, &fields, &body);
        Version {
            id: VersionId::of(&text),
            slug,
            block,
            fields,
            body,
        }
    }

    fn render(slug: &Slug, block: &VersionBlock, fields: &Fields, body: &str) -> String {
        frontmatter::emit(&envelope(slug, fields, block.to_value()), body)
    }

    #[must_use]
    pub fn to_text(&self) -> String {
        Version::render(&self.slug, &self.block, &self.fields, &self.body)
    }

    /// The kind's fields and the body, as `show` prints them.
    #[must_use]
    pub fn content_text(&self) -> String {
        frontmatter::emit(&self.fields, &self.body)
    }

    /// Reads a version from its bytes, refusing any that would not hash to
    /// their own name when written again.
    pub fn from_text(text: &str) -> Result<Version, VersionError> {
        let split = frontmatter::parse(text)?;
        let (slug, block, fields) = open_envelope(split.fields)?;
        let block = match block {
            Value::Map(entries) => VersionBlock::from_entries(&entries)?,
            _ => return Err(FieldError::Missing("version").into()),
        };
        if Version::render(&slug, &block, &fields, &split.body) != text {
            return Err(VersionError::NotCanonical);
        }
        Ok(Version {
            id: VersionId::of(text),
            slug,
            block,
            fields,
            body: split.body,
        })
    }

    /// `from_text` plus the check that the file's name is the hash.
    pub fn from_named_text(name: &str, text: &str) -> Result<Version, VersionError> {
        let version = Version::from_text(text)?;
        if version.id.as_str() != name {
            return Err(VersionError::IdMismatch {
                named: name.to_owned(),
                actual: version.id.0,
            });
        }
        Ok(version)
    }

    #[must_use]
    pub fn is_tombstone(&self) -> bool {
        match self.block.operation {
            Operation::Tombstone => true,
            Operation::Rename => self.block.superseded_by.is_some(),
            _ => false,
        }
    }

    /// The slugs a rename moved the document between, for either version
    /// it wrote: the tombstone names its successor, the moved version's
    /// parent is the tombstone. A moved version without a parent knows
    /// only where it landed.
    #[must_use]
    pub fn rename_sides(&self, parent: Option<&Version>) -> Option<(Option<Slug>, Slug)> {
        if self.block.operation != Operation::Rename {
            return None;
        }
        Some(match &self.block.superseded_by {
            Some(to) => (Some(self.slug.clone()), to.clone()),
            None => (parent.map(|p| p.slug.clone()), self.slug.clone()),
        })
    }
}

/// Every version of one slug.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub versions: Vec<Version>,
}

/// What a document is right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State<'a> {
    /// No version exists.
    Absent,
    Live(&'a Version),
    Tombstoned(&'a Version),
    /// Two or more heads, sorted by id.
    Forked(Vec<&'a Version>),
}

impl Document {
    #[must_use]
    pub fn new(versions: Vec<Version>) -> Document {
        Document { versions }
    }

    #[must_use]
    pub fn get(&self, id: &VersionId) -> Option<&Version> {
        self.versions.iter().find(|v| &v.id == id)
    }

    /// The ids in `history` order, newest first, owned so the document can go.
    #[must_use]
    pub fn history_ids(&self) -> Vec<VersionId> {
        self.history().into_iter().map(|v| v.id.clone()).collect()
    }

    /// Versions no sibling names as a parent, sorted by id.
    #[must_use]
    pub fn heads(&self) -> Vec<&Version> {
        let mut heads: Vec<&Version> = self
            .versions
            .iter()
            .filter(|v| {
                !self
                    .versions
                    .iter()
                    .any(|other| other.block.parents.contains(&v.id))
            })
            .collect();
        heads.sort_by(|a, b| a.id.cmp(&b.id));
        heads
    }

    #[must_use]
    pub fn state(&self) -> State<'_> {
        let heads = self.heads();
        match heads.as_slice() {
            [] => State::Absent,
            [head] if head.is_tombstone() => State::Tombstoned(head),
            [head] => State::Live(head),
            _ => State::Forked(heads),
        }
    }

    /// The slug a rename moved the document to.
    #[must_use]
    pub fn renamed_to(&self) -> Option<&Slug> {
        match self.state() {
            State::Tombstoned(v) => v.block.superseded_by.as_ref(),
            _ => None,
        }
    }

    /// The single live head.
    #[must_use]
    pub fn current(&self) -> Option<&Version> {
        match self.state() {
            State::Live(v) => Some(v),
            _ => None,
        }
    }

    /// Newest first: a version comes after every version naming it as a
    /// parent. Versions on a fork share no order, so the id decides there.
    #[must_use]
    pub fn history(&self) -> Vec<&Version> {
        let mut remaining: Vec<&Version> = self.versions.iter().collect();
        let mut ordered: Vec<&Version> = Vec::new();
        // Hash-linked parents cannot form a cycle, so a childless version
        // always exists while any remain.
        while let Some(next) = remaining
            .iter()
            .enumerate()
            .filter(|(_, v)| {
                !remaining
                    .iter()
                    .any(|child| child.block.parents.contains(&v.id))
            })
            .max_by(|(_, a), (_, b)| a.id.cmp(&b.id))
            .map(|(i, _)| i)
        {
            ordered.push(remaining.remove(next));
        }
        ordered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(parents: &[&VersionId], op: Operation) -> VersionBlock {
        VersionBlock {
            parents: parents.iter().map(|p| (*p).clone()).collect(),
            written: "2026-09-04T10:00:00+01:00".into(),
            machine: MachineName::parse("m1").unwrap(),
            operation: op,
            superseded_by: None,
        }
    }

    fn topic(name: &str, body: &str, parents: &[&VersionId], op: Operation) -> Version {
        let mut fields = Fields::default();
        fields.push_scalar("summary", "a topic");
        Version::compose(
            Slug::parse(name).unwrap(),
            block(parents, op),
            fields,
            body.to_owned(),
        )
    }

    #[test]
    fn text_round_trips_and_names_itself() {
        let v = topic("lantern", "\nbody\n", &[], Operation::New);
        let text = v.to_text();
        assert!(text.starts_with(
            "---\nslug: lantern\nkind: topic\nsummary: a topic\nversion:\n  parents: []\n"
        ));
        assert_eq!(Version::from_named_text(v.id.as_str(), &text).unwrap(), v);
    }

    #[test]
    fn non_canonical_bytes_are_refused() {
        let v = topic("lantern", "\nbody\n", &[], Operation::New);
        let text = v.to_text().replace("summary: a topic", "summary:  a topic");
        assert_eq!(Version::from_text(&text), Err(VersionError::NotCanonical));
    }

    #[test]
    fn heads_and_forks() {
        let a = topic("t", "\n1\n", &[], Operation::New);
        let b = topic("t", "\n2\n", &[&a.id], Operation::Save);
        let c = topic("t", "\n3\n", &[&a.id], Operation::Save);
        let linear = Document::new(vec![a.clone(), b.clone()]);
        assert_eq!(linear.state(), State::Live(&b));
        let forked = Document::new(vec![a.clone(), b.clone(), c.clone()]);
        assert!(matches!(forked.state(), State::Forked(heads) if heads.len() == 2));
        let resolved = topic("t", "\n4\n", &[&b.id, &c.id], Operation::Resolve);
        let doc = Document::new(vec![a.clone(), b, c, resolved.clone()]);
        assert_eq!(doc.state(), State::Live(&resolved));
        assert_eq!(doc.history().len(), 4);
        assert_eq!(doc.history()[0], &resolved);
        assert_eq!(doc.history()[3], &a);
    }

    #[test]
    fn a_tombstone_is_a_head_with_no_body() {
        let a = topic("t", "\n1\n", &[], Operation::New);
        let t = topic("t", "", &[&a.id], Operation::Tombstone);
        let doc = Document::new(vec![a, t.clone()]);
        assert_eq!(doc.state(), State::Tombstoned(&t));
        assert_eq!(doc.current(), None);
    }

    #[test]
    fn absent() {
        assert_eq!(Document::default().state(), State::Absent);
    }
}
