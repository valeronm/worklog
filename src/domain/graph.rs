//! Which topics a session in a directory on a host is about.
//!
//! The host's machine topic says where the topics live. The directory's
//! claims are matched closest first, then, only if none matched, the
//! machine's `unclaimed` topics; the machine topic itself always loads.
//! From every root the `includes` edges are walked breadth-first, each
//! topic once, in the order the roots were found.

use super::topic::Topic;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Via {
    /// Claimed for this directory; the matched path as written in the claim.
    Claim(String),
    Machine,
    Unclaimed,
    Included {
        from: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reached {
    pub topic: String,
    /// Include steps from the root that reached it.
    pub distance: usize,
    pub via: Via,
}

/// A topic by slug; `None` for a name no topic carries.
pub type Lookup<'a> = dyn Fn(&str) -> Option<&'a Topic> + 'a;

fn expand(path: &str, home: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", home.trim_end_matches('/')),
        None if path == "~" => home.trim_end_matches('/').to_owned(),
        None => path.to_owned(),
    }
}

/// How much of `cwd` a claim covers, or nothing.
fn matched(cwd: &str, claim: &str, family: bool, home: &str) -> Option<usize> {
    let claim = expand(claim, home);
    if family {
        return cwd.starts_with(&claim).then_some(claim.len());
    }
    let dir = claim.trim_end_matches('/');
    (cwd == dir || cwd.starts_with(&format!("{dir}/"))).then_some(dir.len())
}

/// Walks `includes` breadth-first from `root`, adding every topic not yet
/// in `reached`; an include naming no topic is skipped here and reported
/// by the store check.
fn walk(root: &str, topics: &Lookup, reached: &mut Vec<Reached>) {
    let mut frontier = vec![(root.to_owned(), 0usize)];
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for (from, distance) in frontier {
            let Some(topic) = topics(&from) else { continue };
            for include in &topic.includes {
                if reached.iter().any(|r| &r.topic == include) || topics(include).is_none() {
                    continue;
                }
                reached.push(Reached {
                    topic: include.clone(),
                    distance: distance + 1,
                    via: Via::Included { from: from.clone() },
                });
                next.push((include.clone(), distance + 1));
            }
        }
        frontier = next;
    }
}

/// The topic and everything its includes reach, nearest first.
#[must_use]
pub fn included(root: &str, topics: &Lookup) -> Vec<Reached> {
    let mut reached = vec![Reached {
        topic: root.to_owned(),
        distance: 0,
        via: Via::Machine,
    }];
    walk(root, topics, &mut reached);
    reached
}

/// The topics for a session in `cwd`, in the order an index lists them.
///
/// `machine` is the host's own topic, if the store has one.
#[must_use]
pub fn resolve(
    machine: Option<(&str, &Topic)>,
    cwd: &str,
    home: &str,
    topics: &Lookup,
) -> Vec<Reached> {
    let cwd = cwd.trim_end_matches('/');
    let mut roots: Vec<(String, Via)> = Vec::new();
    if let Some((machine_slug, machine)) = machine {
        // Longest match per topic, then topics by match length, then name.
        let mut best: Vec<(&str, usize, &str)> = Vec::new();
        let claims = machine.claims.iter().map(|(t, paths)| (t, paths, false));
        let families = machine.families.iter().map(|(t, paths)| (t, paths, true));
        for (topic, paths, family) in claims.chain(families) {
            for path in paths {
                let Some(len) = matched(cwd, path, family, home) else {
                    continue;
                };
                match best.iter_mut().find(|(t, _, _)| *t == topic) {
                    Some(slot) if slot.1 >= len => {}
                    Some(slot) => *slot = (topic, len, path),
                    None => best.push((topic, len, path)),
                }
            }
        }
        best.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        for (topic, _, path) in &best {
            roots.push(((*topic).to_owned(), Via::Claim((*path).to_owned())));
        }
        roots.push((machine_slug.to_owned(), Via::Machine));
        if best.is_empty() {
            for topic in &machine.unclaimed {
                roots.push((topic.clone(), Via::Unclaimed));
            }
        }
    }
    let mut reached: Vec<Reached> = Vec::new();
    for (root, via) in roots {
        if reached.iter().any(|r| r.topic == root) {
            continue;
        }
        reached.push(Reached {
            topic: root.clone(),
            distance: 0,
            via,
        });
        walk(&root, topics, &mut reached);
    }
    reached
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn topic(includes: &[&str]) -> Topic {
        Topic {
            summary: "t".into(),
            includes: includes.iter().map(|s| (*s).to_owned()).collect(),
            ..Topic::default()
        }
    }

    fn store() -> BTreeMap<String, Topic> {
        let mut topics = BTreeMap::new();
        topics.insert("atlas".into(), topic(&["android"]));
        topics.insert("android".into(), topic(&["phone-a", "phone-b"]));
        topics.insert("phone-a".into(), topic(&[]));
        topics.insert("phone-b".into(), topic(&["android"]));
        topics.insert("personal".into(), topic(&["taxes"]));
        topics.insert("taxes".into(), topic(&[]));
        topics.insert("lab".into(), topic(&[]));
        topics.insert(
            "host".into(),
            Topic {
                claims: vec![("atlas".into(), vec!["~/projects/Android/atlas".into()])],
                families: vec![("lab".into(), vec!["~/projects/lab-".into()])],
                unclaimed: vec!["personal".into()],
                ..topic(&[])
            },
        );
        topics
    }

    fn names(reached: &[Reached]) -> Vec<(&str, usize)> {
        reached
            .iter()
            .map(|r| (r.topic.as_str(), r.distance))
            .collect()
    }

    fn resolve_in(topics: &BTreeMap<String, Topic>, cwd: &str) -> Vec<Reached> {
        let host = topics.get("host").unwrap();
        resolve(Some(("host", host)), cwd, "/home/u", &|name| {
            topics.get(name)
        })
    }

    #[test]
    fn a_claimed_directory_walks_includes_then_the_machine() {
        let topics = store();
        let reached = resolve_in(&topics, "/home/u/projects/Android/atlas/app");
        assert_eq!(
            names(&reached),
            [
                ("atlas", 0),
                ("android", 1),
                ("phone-a", 2),
                ("phone-b", 2),
                ("host", 0)
            ]
        );
        assert_eq!(
            reached[0].via,
            Via::Claim("~/projects/Android/atlas".into())
        );
        assert_eq!(
            reached[1].via,
            Via::Included {
                from: "atlas".into()
            }
        );
    }

    #[test]
    fn an_unclaimed_directory_gets_the_machine_chain() {
        let topics = store();
        let reached = resolve_in(&topics, "/home/u/Documents");
        assert_eq!(
            names(&reached),
            [("host", 0), ("personal", 0), ("taxes", 1)]
        );
        assert_eq!(reached[1].via, Via::Unclaimed);
    }

    #[test]
    fn families_match_by_prefix_and_dirs_by_segment() {
        let topics = store();
        let reached = resolve_in(&topics, "/home/u/projects/lab-sensors");
        assert_eq!(names(&reached)[0], ("lab", 0));
        let reached = resolve_in(&topics, "/home/u/projects/Android/atlas-old");
        assert_eq!(
            names(&reached),
            [("host", 0), ("personal", 0), ("taxes", 1)]
        );
    }

    #[test]
    fn included_walks_from_one_topic() {
        let topics = store();
        let reached = included("atlas", &|name| topics.get(name));
        assert_eq!(
            names(&reached),
            [("atlas", 0), ("android", 1), ("phone-a", 2), ("phone-b", 2)]
        );
    }

    #[test]
    fn no_machine_topic_reaches_nothing() {
        let topics = store();
        assert!(
            resolve(None, "/home/u/projects/Android/atlas", "/home/u", &|name| {
                topics.get(name)
            })
            .is_empty()
        );
    }
}
