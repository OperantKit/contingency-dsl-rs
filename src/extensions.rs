//! Extension mechanism for contingency-dsl.
//!
//! Mirrors `contingency_dsl/extensions.py` in the Python reference
//! implementation. Defines the `AnnotationExtension` trait and an
//! `ExtensionRegistry` that composes extensions and validates
//! keyword-disjointness and dependency acyclicity.
//!
//! Only the annotation dimension is ported here; the primitive-dimension
//! hook (identifier-headed respondent primitives) is handled separately
//! via `RespondentExtensionCall` in the AST.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::ast::Annotation;

/// Contract for a DSL annotation extension.
///
/// Implementors contribute one or more `@`-prefixed annotation keywords
/// (without the `@` prefix) and provide semantic validation for their
/// keywords' parameter shapes.
pub trait AnnotationExtension: Send + Sync {
    /// Stable name (e.g., `"stimulus-ext"`).
    fn name(&self) -> &'static str;

    /// Extension version string.
    fn version(&self) -> &'static str;

    /// Reserved annotation keywords contributed by this extension.
    ///
    /// The `@` prefix is implicit; return bare names.
    fn annotation_keywords(&self) -> &'static [&'static str];

    /// Names of other extensions this extension depends on.
    fn requires(&self) -> &'static [&'static str] {
        &[]
    }

    /// Semantic validation of a slice of parsed annotations.
    ///
    /// Returns a list of diagnostic strings (empty = valid). Each diagnostic
    /// mirrors a Python counterpart byte-for-byte where feasible.
    fn validate(&self, annotations: &[Annotation]) -> Vec<String>;
}

/// Error raised when extension dependencies form a cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CyclicDependencyError {
    pub cycle: Vec<String>,
}

impl std::fmt::Display for CyclicDependencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cyclic dependency detected: {}", self.cycle.join(" -> "))
    }
}

impl std::error::Error for CyclicDependencyError {}

/// Error raised when two extensions claim the same annotation keyword.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordConflictError {
    pub keyword: String,
    pub first: String,
    pub second: String,
}

impl std::fmt::Display for KeywordConflictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "annotation keyword '@{}' claimed by both '{}' and '{}'",
            self.keyword, self.first, self.second
        )
    }
}

impl std::error::Error for KeywordConflictError {}

/// Composition error for registry construction.
#[derive(Debug)]
pub enum RegistryError {
    DuplicateExtension(String),
    KeywordConflict(KeywordConflictError),
    MissingDependency { extension: String, missing: String },
    Cycle(CyclicDependencyError),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateExtension(name) => write!(f, "duplicate extension name: '{name}'"),
            Self::KeywordConflict(err) => write!(f, "{err}"),
            Self::MissingDependency { extension, missing } => write!(
                f,
                "extension '{extension}' requires '{missing}' but it is not registered"
            ),
            Self::Cycle(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Composed registry of annotation extensions.
pub struct ExtensionRegistry {
    extensions: Vec<Box<dyn AnnotationExtension>>,
    all_keywords: BTreeSet<String>,
    keyword_owner: BTreeMap<String, usize>,
}

impl ExtensionRegistry {
    /// Build a registry from a vector of boxed extensions.
    ///
    /// Fails if two extensions claim the same keyword, if a declared
    /// dependency is missing, or if the dependency graph contains a cycle.
    pub fn build(
        extensions: Vec<Box<dyn AnnotationExtension>>,
    ) -> Result<Self, RegistryError> {
        // 1. Duplicate names
        let mut names: BTreeSet<String> = BTreeSet::new();
        for ext in &extensions {
            let name = ext.name().to_string();
            if !names.insert(name.clone()) {
                return Err(RegistryError::DuplicateExtension(name));
            }
        }

        // 2. Keyword disjointness
        let mut all_keywords: BTreeSet<String> = BTreeSet::new();
        let mut keyword_owner: BTreeMap<String, usize> = BTreeMap::new();
        for (idx, ext) in extensions.iter().enumerate() {
            for kw in ext.annotation_keywords() {
                if let Some(&other_idx) = keyword_owner.get(*kw) {
                    return Err(RegistryError::KeywordConflict(KeywordConflictError {
                        keyword: kw.to_string(),
                        first: extensions[other_idx].name().to_string(),
                        second: ext.name().to_string(),
                    }));
                }
                all_keywords.insert(kw.to_string());
                keyword_owner.insert(kw.to_string(), idx);
            }
        }

        // 3. Dependency satisfaction
        for ext in &extensions {
            for dep in ext.requires() {
                if !names.contains(*dep) {
                    return Err(RegistryError::MissingDependency {
                        extension: ext.name().to_string(),
                        missing: (*dep).to_string(),
                    });
                }
            }
        }

        // 4. Acyclicity — Kahn's algorithm on the `requires` edges.
        //    (Edge: ext -> dep, meaning ext depends on dep. In-degree of dep
        //     counts how many extensions require it.)
        let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for ext in &extensions {
            graph
                .entry(ext.name().to_string())
                .or_default()
                .extend(ext.requires().iter().map(|s| s.to_string()));
        }
        let mut in_degree: BTreeMap<String, usize> = BTreeMap::new();
        for name in graph.keys() {
            in_degree.entry(name.clone()).or_insert(0);
        }
        for deps in graph.values() {
            for dep in deps {
                *in_degree.entry(dep.clone()).or_insert(0) += 1;
            }
        }
        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(n, _)| n.clone())
            .collect();
        let mut visited = 0usize;
        while let Some(node) = queue.pop_front() {
            visited += 1;
            if let Some(deps) = graph.get(&node) {
                for dep in deps {
                    if let Some(d) = in_degree.get_mut(dep) {
                        *d -= 1;
                        if *d == 0 {
                            queue.push_back(dep.clone());
                        }
                    }
                }
            }
        }
        if visited != graph.len() {
            let cycle = find_cycle(&graph);
            return Err(RegistryError::Cycle(CyclicDependencyError { cycle }));
        }

        Ok(Self {
            extensions,
            all_keywords,
            keyword_owner,
        })
    }

    /// All annotation keywords registered by this registry.
    pub fn all_annotation_keywords(&self) -> &BTreeSet<String> {
        &self.all_keywords
    }

    /// True if the keyword is claimed by some registered extension.
    pub fn has_keyword(&self, keyword: &str) -> bool {
        self.all_keywords.contains(keyword)
    }

    /// Extensions, in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &dyn AnnotationExtension> {
        self.extensions.iter().map(|e| e.as_ref())
    }

    /// Look up an extension by name.
    pub fn get(&self, name: &str) -> Option<&dyn AnnotationExtension> {
        self.extensions
            .iter()
            .find(|e| e.name() == name)
            .map(|e| e.as_ref())
    }

    /// Run every extension's `validate` against the provided annotations.
    /// Each extension sees only the annotations whose keyword it owns.
    pub fn validate(&self, annotations: &[Annotation]) -> Vec<String> {
        let mut diagnostics: Vec<String> = Vec::new();
        for (idx, ext) in self.extensions.iter().enumerate() {
            let owned: Vec<Annotation> = annotations
                .iter()
                .filter(|a| self.keyword_owner.get(&a.keyword) == Some(&idx))
                .cloned()
                .collect();
            diagnostics.extend(ext.validate(&owned));
        }
        diagnostics
    }
}

fn find_cycle(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    let mut color: BTreeMap<String, Color> = graph.keys().map(|k| (k.clone(), Color::White)).collect();
    let mut parent: BTreeMap<String, Option<String>> =
        graph.keys().map(|k| (k.clone(), None)).collect();

    for start in graph.keys() {
        if color[start] != Color::White {
            continue;
        }
        let mut stack: Vec<String> = vec![start.clone()];
        while let Some(node) = stack.last().cloned() {
            match color[&node] {
                Color::White => {
                    color.insert(node.clone(), Color::Gray);
                    if let Some(neighbors) = graph.get(&node) {
                        for neighbor in neighbors {
                            match color.get(neighbor).copied().unwrap_or(Color::White) {
                                Color::Gray => {
                                    // Reconstruct cycle
                                    let mut cycle = vec![neighbor.clone(), node.clone()];
                                    let mut cur = node.clone();
                                    while cur != *neighbor {
                                        match parent.get(&cur).cloned().flatten() {
                                            Some(p) => {
                                                cycle.push(p.clone());
                                                cur = p;
                                            }
                                            None => break,
                                        }
                                    }
                                    cycle.reverse();
                                    cycle.push(cycle[0].clone());
                                    return cycle;
                                }
                                Color::White => {
                                    parent.insert(neighbor.clone(), Some(node.clone()));
                                    stack.push(neighbor.clone());
                                }
                                Color::Black => {}
                            }
                        }
                    }
                }
                _ => {
                    color.insert(node.clone(), Color::Black);
                    stack.pop();
                }
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyExt {
        name: &'static str,
        kws: &'static [&'static str],
        deps: &'static [&'static str],
    }

    impl AnnotationExtension for DummyExt {
        fn name(&self) -> &'static str {
            self.name
        }
        fn version(&self) -> &'static str {
            "0.0.0"
        }
        fn annotation_keywords(&self) -> &'static [&'static str] {
            self.kws
        }
        fn requires(&self) -> &'static [&'static str] {
            self.deps
        }
        fn validate(&self, _: &[Annotation]) -> Vec<String> {
            Vec::new()
        }
    }

    #[test]
    fn build_collects_keywords() {
        let reg = ExtensionRegistry::build(vec![
            Box::new(DummyExt {
                name: "a",
                kws: &["x", "y"],
                deps: &[],
            }),
            Box::new(DummyExt {
                name: "b",
                kws: &["z"],
                deps: &[],
            }),
        ])
        .unwrap();
        assert!(reg.has_keyword("x"));
        assert!(reg.has_keyword("y"));
        assert!(reg.has_keyword("z"));
        assert_eq!(reg.all_annotation_keywords().len(), 3);
    }

    #[test]
    fn build_rejects_keyword_collision() {
        let res = ExtensionRegistry::build(vec![
            Box::new(DummyExt {
                name: "a",
                kws: &["x"],
                deps: &[],
            }),
            Box::new(DummyExt {
                name: "b",
                kws: &["x"],
                deps: &[],
            }),
        ]);
        assert!(matches!(res, Err(RegistryError::KeywordConflict(_))));
    }

    #[test]
    fn build_rejects_missing_dep() {
        let res = ExtensionRegistry::build(vec![Box::new(DummyExt {
            name: "a",
            kws: &[],
            deps: &["ghost"],
        })]);
        assert!(matches!(res, Err(RegistryError::MissingDependency { .. })));
    }

    #[test]
    fn build_rejects_cycle() {
        let res = ExtensionRegistry::build(vec![
            Box::new(DummyExt {
                name: "a",
                kws: &[],
                deps: &["b"],
            }),
            Box::new(DummyExt {
                name: "b",
                kws: &[],
                deps: &["a"],
            }),
        ]);
        assert!(matches!(res, Err(RegistryError::Cycle(_))));
    }

    #[test]
    fn build_rejects_duplicate_name() {
        let res = ExtensionRegistry::build(vec![
            Box::new(DummyExt {
                name: "a",
                kws: &[],
                deps: &[],
            }),
            Box::new(DummyExt {
                name: "a",
                kws: &["z"],
                deps: &[],
            }),
        ]);
        assert!(matches!(res, Err(RegistryError::DuplicateExtension(_))));
    }
}
