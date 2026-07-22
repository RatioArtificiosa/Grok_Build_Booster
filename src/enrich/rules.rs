use crate::state::TopicCategory;

/// Keyword / heuristic categorization (offline, zero model cost).
pub fn categorize_rules(prompt: &str) -> TopicCategory {
    let p = prompt.to_lowercase();
    let checks: &[(TopicCategory, &[&str])] = &[
        (
            TopicCategory::Security,
            &[
                "auth",
                "oauth",
                "jwt",
                "security",
                "xss",
                "csrf",
                "secret",
                "password",
                "encrypt",
                "vulnerability",
                "cve",
            ],
        ),
        (
            TopicCategory::Database,
            &[
                "sql",
                "database",
                "postgres",
                "mysql",
                "mongo",
                "migration",
                "schema",
                "query",
                "prisma",
                "drizzle",
            ],
        ),
        (
            TopicCategory::Ui,
            &[
                "ui",
                "css",
                "layout",
                "button",
                "sidebar",
                "dashboard",
                "frontend",
                "react",
                "tailwind",
                "design",
                "tui",
                "ratatui",
            ],
        ),
        (
            TopicCategory::Tests,
            &[
                "test",
                "spec",
                "assert",
                "coverage",
                "jest",
                "pytest",
                "unit test",
                "e2e",
                "integration test",
            ],
        ),
        (
            TopicCategory::Api,
            &[
                "api",
                "endpoint",
                "rest",
                "graphql",
                "http",
                "route",
                "handler",
                "webhook",
            ],
        ),
        (
            TopicCategory::Config,
            &[
                "config",
                "env",
                "toml",
                "yaml",
                "settings",
                "feature flag",
                "dotenv",
            ],
        ),
        (
            TopicCategory::Refactor,
            &[
                "refactor",
                "cleanup",
                "rename",
                "extract",
                "simplify",
                "dedupe",
                "restructure",
            ],
        ),
        (
            TopicCategory::Devops,
            &[
                "docker",
                "ci",
                "cd",
                "deploy",
                "kubernetes",
                "k8s",
                "terraform",
                "pipeline",
                "github actions",
                "infra",
            ],
        ),
        (
            TopicCategory::Docs,
            &[
                "docs",
                "readme",
                "documentation",
                "comment",
                "changelog",
                "guide",
            ],
        ),
    ];

    for (cat, kws) in checks {
        if kws.iter().any(|k| p.contains(k)) {
            return *cat;
        }
    }
    TopicCategory::Other
}

/// Cheap 3-word title: first meaningful tokens of the prompt.
pub fn three_word_summary_rules(prompt: &str) -> String {
    let stop: &[&str] = &[
        "a", "an", "the", "to", "and", "or", "of", "in", "on", "for", "with", "please", "can",
        "you", "we", "i", "me", "my", "this", "that", "is", "are", "be", "do", "make", "add",
    ];
    let words: Vec<&str> = prompt
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_'))
        .filter(|w| !w.is_empty())
        .filter(|w| !stop.contains(&w.to_lowercase().as_str()))
        .take(3)
        .collect();

    if words.is_empty() {
        "New prompt".into()
    } else {
        words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_security() {
        assert_eq!(
            categorize_rules("fix the oauth login flow"),
            TopicCategory::Security
        );
    }

    #[test]
    fn summary_is_short() {
        let s = three_word_summary_rules("Please add a dark mode toggle to the settings page");
        assert!(s.split_whitespace().count() <= 3);
        assert!(!s.is_empty());
    }
}
