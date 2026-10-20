// SPDX-License-Identifier: GPL-3.0-or-later
//! Architecture fitness check: crates may only depend inward (ARCHITECTURE §2 "Layering rule").

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Normal,
    Dev,
    Build,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dep {
    pub name: String,
    pub kind: Kind,
    /// Path dependency, i.e. another workspace crate (not a crates.io package).
    pub internal: bool,
}

/// Dependencies of each workspace crate, by crate name.
#[derive(Debug, Default, PartialEq)]
pub struct Graph {
    pub crates: BTreeMap<String, Vec<Dep>>,
}

pub struct Rules {
    /// Workspace crates each crate may depend on (normal, build and dev dependencies alike).
    pub internal: BTreeMap<&'static str, &'static [&'static str]>,
    /// Crates whose normal dependencies are limited to an external allowlist.
    pub external_allowlist: BTreeMap<&'static str, &'static [&'static str]>,
    pub ignored: &'static [&'static str],
}

#[derive(Debug, PartialEq)]
pub enum Violation {
    NoRule {
        krate: String,
    },
    Upward {
        krate: String,
        dep: String,
        allowed: &'static [&'static str],
    },
    Impure {
        krate: String,
        dep: String,
        allowlist: &'static [&'static str],
    },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::NoRule { krate } => {
                write!(
                    f,
                    "{krate}: no layering rule; add one to kipple_rules() in xtask/src/layering.rs"
                )
            }
            Violation::Upward {
                krate,
                dep,
                allowed,
            } => {
                write!(f, "{krate} must not depend on {dep} (allowed: {allowed:?})")
            }
            Violation::Impure {
                krate,
                dep,
                allowlist,
            } => {
                write!(
                    f,
                    "{krate} must stay pure: {dep} is not on its allowlist {allowlist:?}"
                )
            }
        }
    }
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    dependencies: Vec<MetaDep>,
}

#[derive(Deserialize)]
struct MetaDep {
    name: String,
    kind: Option<String>,
    path: Option<String>,
}

/// Parses `cargo metadata --format-version 1 --no-deps` output (which lists only workspace
/// packages) into a [`Graph`].
pub fn from_metadata(json: &str) -> Result<Graph, String> {
    let metadata: Metadata = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut graph = Graph::default();
    for package in metadata.packages {
        let mut deps: Vec<Dep> = package
            .dependencies
            .into_iter()
            .map(|d| Dep {
                kind: match d.kind.as_deref() {
                    Some("dev") => Kind::Dev,
                    Some("build") => Kind::Build,
                    _ => Kind::Normal,
                },
                internal: d.path.is_some(),
                name: d.name,
            })
            .collect();
        deps.sort();
        deps.dedup();
        graph.crates.insert(package.name, deps);
    }
    Ok(graph)
}

/// Returns every rule violation in `graph`.
pub fn check(graph: &Graph, rules: &Rules) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (krate, deps) in &graph.crates {
        if rules.ignored.contains(&krate.as_str()) {
            continue;
        }
        let Some(&allowed) = rules.internal.get(krate.as_str()) else {
            violations.push(Violation::NoRule {
                krate: krate.clone(),
            });
            continue;
        };
        let allowlist = rules.external_allowlist.get(krate.as_str()).copied();
        for dep in deps {
            if dep.internal {
                // Inward-only applies to every kind: a dev-dependency can't sneak a cycle in.
                if !allowed.contains(&dep.name.as_str()) {
                    violations.push(Violation::Upward {
                        krate: krate.clone(),
                        dep: dep.name.clone(),
                        allowed,
                    });
                }
            } else if let Some(allowlist) = allowlist {
                // Tests and build scripts may use I/O helpers; only runtime code must stay pure.
                if dep.kind == Kind::Normal && !allowlist.contains(&dep.name.as_str()) {
                    violations.push(Violation::Impure {
                        krate: krate.clone(),
                        dep: dep.name.clone(),
                        allowlist,
                    });
                }
            }
        }
    }
    violations
}

const NONE: &[&str] = &[];

/// Kipple's layering rules (ARCHITECTURE §2, §4). Dependencies only point inward.
pub fn kipple_rules() -> Rules {
    Rules {
        internal: BTreeMap::from([
            ("kipple-core", NONE),
            ("kipple-i18n-rs", NONE),
            ("kipple-index", &["kipple-core"][..]),
            ("kipple-platform", &["kipple-core"][..]),
            ("kipple-preview", &["kipple-core", "kipple-platform"][..]),
            (
                "kipple-engine",
                &[
                    "kipple-core",
                    "kipple-index",
                    "kipple-platform",
                    "kipple-preview",
                ][..],
            ),
            (
                "kipple-cli",
                &[
                    "kipple-core",
                    "kipple-index",
                    "kipple-platform",
                    "kipple-preview",
                    "kipple-engine",
                ][..],
            ),
            (
                "kipple-app",
                &[
                    "kipple-core",
                    "kipple-index",
                    "kipple-platform",
                    "kipple-preview",
                    "kipple-engine",
                    "kipple-i18n-rs",
                ][..],
            ),
        ]),
        // Pure, I/O-free crates only. Adding one is a deliberate, reviewed change.
        external_allowlist: BTreeMap::from([("kipple-core", &["serde", "thiserror"][..])]),
        ignored: &["xtask"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dep(name: &str, kind: Kind) -> Dep {
        Dep {
            name: name.to_string(),
            kind,
            internal: name.starts_with("kipple-"),
        }
    }

    fn graph(edges: &[(&str, &[(&str, Kind)])]) -> Graph {
        Graph {
            crates: edges
                .iter()
                .map(|(c, deps)| {
                    (
                        c.to_string(),
                        deps.iter().map(|(n, k)| dep(n, *k)).collect(),
                    )
                })
                .collect(),
        }
    }

    fn violations(edges: &[(&str, &[(&str, Kind)])]) -> Vec<Violation> {
        check(&graph(edges), &kipple_rules())
    }

    use Kind::{Build, Dev, Normal};

    #[test]
    fn core_may_use_allowlisted_pure_crates() {
        assert_eq!(
            violations(&[("kipple-core", &[("serde", Normal), ("thiserror", Normal)])]),
            vec![]
        );
    }

    #[test]
    fn core_must_not_depend_on_other_workspace_crates() {
        let v = violations(&[
            ("kipple-core", &[("kipple-index", Normal)]),
            ("kipple-index", &[("kipple-core", Normal)]),
        ]);
        assert!(
            matches!(&v[..], [Violation::Upward { krate, dep, .. }] if krate == "kipple-core" && dep == "kipple-index"),
            "{v:?}"
        );
    }

    #[test]
    fn core_must_not_use_io_crates() {
        let v = violations(&[("kipple-core", &[("tokio", Normal)])]);
        assert!(
            matches!(&v[..], [Violation::Impure { dep, .. }] if dep == "tokio"),
            "{v:?}"
        );
    }

    #[test]
    fn core_tests_and_build_scripts_may_use_io_crates() {
        assert_eq!(
            violations(&[("kipple-core", &[("tempfile", Dev), ("cc", Build)])]),
            vec![]
        );
    }

    #[test]
    fn dev_dependencies_must_also_point_inward() {
        let v = violations(&[
            ("kipple-core", &[("kipple-engine", Dev)]),
            ("kipple-engine", &[]),
        ]);
        assert!(
            matches!(&v[..], [Violation::Upward { dep, .. }] if dep == "kipple-engine"),
            "{v:?}"
        );
    }

    #[test]
    fn engine_may_depend_inward() {
        let v = violations(&[
            ("kipple-core", &[]),
            (
                "kipple-index",
                &[("kipple-core", Normal), ("rusqlite", Normal)],
            ),
            ("kipple-platform", &[("kipple-core", Normal)]),
            (
                "kipple-engine",
                &[
                    ("kipple-core", Normal),
                    ("kipple-index", Normal),
                    ("kipple-platform", Normal),
                    ("tokio", Normal),
                ],
            ),
        ]);
        assert_eq!(v, vec![]);
    }

    #[test]
    fn upward_dependencies_are_rejected() {
        let v = violations(&[
            (
                "kipple-index",
                &[("kipple-core", Normal), ("kipple-engine", Normal)],
            ),
            ("kipple-engine", &[]),
        ]);
        assert!(
            matches!(&v[..], [Violation::Upward { krate, dep, .. }] if krate == "kipple-index" && dep == "kipple-engine"),
            "{v:?}"
        );
    }

    #[test]
    fn crates_without_a_rule_are_rejected() {
        let v = violations(&[("kipple-mystery", &[])]);
        assert!(
            matches!(&v[..], [Violation::NoRule { krate }] if krate == "kipple-mystery"),
            "{v:?}"
        );
    }

    #[test]
    fn ignored_crates_are_not_checked() {
        assert_eq!(
            violations(&[("xtask", &[("kipple-engine", Normal), ("anything", Normal)])]),
            vec![]
        );
    }

    #[test]
    fn metadata_tracks_kind_and_path_dependencies() {
        let json = r#"{ "packages": [
            { "name": "kipple-engine", "dependencies": [
                { "name": "kipple-core", "kind": null, "path": "/k/crates/kipple-core" },
                { "name": "tokio", "kind": null },
                { "name": "tempfile", "kind": "dev" },
                { "name": "cc", "kind": "build" }
            ] }
        ] }"#;
        let g = from_metadata(json).unwrap();
        assert_eq!(
            g.crates["kipple-engine"],
            vec![
                Dep {
                    name: "cc".into(),
                    kind: Build,
                    internal: false
                },
                Dep {
                    name: "kipple-core".into(),
                    kind: Normal,
                    internal: true
                },
                Dep {
                    name: "tempfile".into(),
                    kind: Dev,
                    internal: false
                },
                Dep {
                    name: "tokio".into(),
                    kind: Normal,
                    internal: false
                },
            ]
        );
    }

    #[test]
    fn a_crates_io_package_named_like_a_rule_is_not_internal() {
        let mut g = graph(&[("kipple-engine", &[])]);
        g.crates.get_mut("kipple-engine").unwrap().push(Dep {
            name: "kipple-app".into(),
            kind: Normal,
            internal: false,
        });
        assert_eq!(check(&g, &kipple_rules()), vec![]);
    }
}
