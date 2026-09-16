use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const LITELLM_PRICING_JSON: &str = include_str!("litellm-pricing-embedded.json");

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPrice {
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub cache_read_per_m: f64,
    pub cache_write_per_m: f64,
}

#[derive(Deserialize)]
struct RawPriceEntry {
    i: f64,
    o: f64,
    cr: Option<f64>,
    cc: Option<f64>,
}

pub struct PricingRegistry {
    entries: HashMap<String, ModelPrice>,
}

impl PricingRegistry {
    pub fn global() -> &'static Self {
        static REGISTRY: OnceLock<PricingRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            let mut entries = HashMap::new();
            if let Ok(raw_map) =
                serde_json::from_str::<HashMap<String, RawPriceEntry>>(LITELLM_PRICING_JSON)
            {
                for (model, raw) in raw_map {
                    let input_per_m = raw.i * 1_000_000.0;
                    let output_per_m = raw.o * 1_000_000.0;
                    let cache_read_per_m =
                        raw.cr.map(|c| c * 1_000_000.0).unwrap_or(input_per_m * 0.1);
                    let cache_write_per_m = raw
                        .cc
                        .map(|c| c * 1_000_000.0)
                        .unwrap_or(input_per_m * 1.25);

                    entries.insert(
                        model.to_lowercase(),
                        ModelPrice {
                            input_per_m,
                            output_per_m,
                            cache_read_per_m,
                            cache_write_per_m,
                        },
                    );
                }
            }
            PricingRegistry { entries }
        })
    }

    pub fn find(&self, model_name: &str) -> Option<ModelPrice> {
        let clean = model_name.trim().to_lowercase();
        if clean.is_empty() {
            return None;
        }

        // 1. Correspondance exacte directe
        if let Some(&p) = self.entries.get(&clean) {
            return Some(p);
        }

        let alias = match clean.as_str() {
            "deepseek/deepseek-v4-pro-0813" => Some("deepseek/deepseek-v4-pro"),
            "moonshotai/kimi-k3" => Some("moonshot/kimi-k3"),
            "openai/gpt-5.6-sol-pro" => Some("openrouter/openai/gpt-5.6-sol-pro"),
            "x-ai/grok-4.6" | "cursor-grok-4.6-medium" => Some("vertex_ai/xai/grok-4.6"),
            _ => None,
        };
        if let Some(&price) = alias.and_then(|name| self.entries.get(name)) {
            return Some(price);
        }

        // 2. Nettoyage des préfixes de fournisseur courants
        let stripped = clean
            .trim_start_matches("google/")
            .trim_start_matches("gemini/")
            .trim_start_matches("openai/")
            .trim_start_matches("anthropic/")
            .trim_start_matches("vertex_ai/")
            .trim_start_matches("openrouter/openai/")
            .trim_start_matches("openrouter/google/")
            .trim_start_matches("x-ai/")
            .trim_start_matches("cursor-");

        if let Some(&p) = self.entries.get(stripped) {
            return Some(p);
        }

        // 3. Correspondance avec préfixe de base de fournisseur
        for prefix in &[
            "google/",
            "vertex_ai/",
            "openai/",
            "anthropic/",
            "deepseek/",
            "moonshot/",
        ] {
            let candidate = format!("{}{}", prefix, stripped);
            if let Some(&p) = self.entries.get(&candidate) {
                return Some(p);
            }
        }

        // 4. Correspondance de variantes (ex: gemini-3.8-flash-high -> gemini-3.8-flash)
        for base in &[
            "gemini-3.8-flash",
            "gemini-3.7-flash",
            "gemini-3.6-flash",
            "gemini-3.5-flash",
            "gemini-3.5-flash-lite",
            "gemini-3.1-pro",
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
            "gemini-2.0-flash",
            "gemini-2.0-flash-lite",
            "gpt-5.5",
            "gpt-5.4",
            "gpt-5.3-codex",
            "gpt-5.2-codex",
            "gpt-5.2",
            "gpt-5.1-codex",
            "gpt-5.1-codex-mini",
            "gpt-5.1",
            "claude-3-7-sonnet",
            "claude-3-5-sonnet",
            "claude-sonnet-4-6",
            "claude-opus-4-6",
        ] {
            if stripped.starts_with(base) {
                if let Some(&p) = self.entries.get(*base) {
                    return Some(p);
                }
            }
        }

        None
    }
}

pub fn get_model_pricing(model_name: &str) -> Option<ModelPrice> {
    PricingRegistry::global().find(model_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_models_pricing_exactness() {
        let p_38 = get_model_pricing("gemini-3.8-flash").unwrap();
        assert_eq!(p_38.input_per_m, 0.75);
        assert_eq!(p_38.output_per_m, 3.75);
        assert_eq!(p_38.cache_read_per_m, 0.075);

        let p_38_variant = get_model_pricing("gemini-3.8-flash-high").unwrap();
        assert_eq!(p_38_variant.input_per_m, 0.75);
        assert_eq!(p_38_variant.output_per_m, 3.75);

        let p_pro = get_model_pricing("gemini-2.5-pro").unwrap();
        assert_eq!(p_pro.input_per_m, 1.25);
        assert_eq!(p_pro.output_per_m, 10.0);
        assert_eq!(p_pro.cache_read_per_m, 0.125);

        let p_gpt = get_model_pricing("gpt-5.5").unwrap();
        assert_eq!(p_gpt.input_per_m, 5.0);
        assert_eq!(p_gpt.output_per_m, 30.0);
        assert_eq!(p_gpt.cache_read_per_m, 0.5);
        assert!(get_model_pricing("unknown-future-model").is_none());
    }

    #[test]
    fn provider_and_variant_aliases_resolve_used_models() {
        assert_eq!(
            get_model_pricing("deepseek/deepseek-v4-pro-0813"),
            get_model_pricing("deepseek/deepseek-v4-pro")
        );
        assert_eq!(
            get_model_pricing("moonshotai/kimi-k3"),
            get_model_pricing("moonshot/kimi-k3")
        );
        assert_eq!(
            get_model_pricing("openai/gpt-5.6-sol-pro"),
            PricingRegistry::global().find("openrouter/openai/gpt-5.6-sol-pro")
        );
        assert_eq!(
            get_model_pricing("x-ai/grok-4.6"),
            PricingRegistry::global().find("vertex_ai/xai/grok-4.6")
        );
        assert_eq!(
            get_model_pricing("cursor-grok-4.6-medium"),
            PricingRegistry::global().find("vertex_ai/xai/grok-4.6")
        );
    }
}
