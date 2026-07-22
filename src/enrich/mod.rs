//! Prompt enrichment: rule-based MVP + optional SuperGrok OAuth remote summary/category.

mod rules;
mod remote;

pub use remote::{enrich_with_remote_grok, Enrichment};
pub use rules::{categorize_rules, three_word_summary_rules};

use crate::oauth::TokenStore;
use crate::state::TopicCategory;

/// Prefer remote SuperGrok when logged in; always fall back to rules on any failure.
pub async fn enrich_prompt(prompt: &str, store: &TokenStore) -> (String, TopicCategory, bool) {
    let rules_summary = three_word_summary_rules(prompt);
    let rules_cat = categorize_rules(prompt);

    if store.load().ok().flatten().is_none() {
        return (rules_summary, rules_cat, false);
    }

    match enrich_with_remote_grok(prompt, store).await {
        Ok(e) => (e.short_desc, e.category, true),
        Err(err) => {
            tracing::warn!("remote enrich failed, using rules: {err:#}");
            (rules_summary, rules_cat, false)
        }
    }
}
