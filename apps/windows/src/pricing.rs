use serde::Deserialize;
use std::collections::HashMap;
use std::{collections::HashMap as StdHashMap, sync::OnceLock};
#[cfg(not(test))]
use std::{env, fs, path::PathBuf, sync::Mutex, time::SystemTime};

const LITELLM_PRICING_JSON: &str = include_str!("litellm-pricing-embedded.json");
#[cfg(not(test))]
const MAX_DYNAMIC_PRICING_BYTES: u64 = 64 * 1024 * 1024;

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
        let mut variant_bases = [
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
        ];
        variant_bases.sort_unstable_by_key(|base| std::cmp::Reverse(base.len()));
        for base in &variant_bases {
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
    #[cfg(not(test))]
    {
        dynamic_cached_price(model_name).or_else(|| PricingRegistry::global().find(model_name))
    }
    #[cfg(test)]
    {
        PricingRegistry::global().find(model_name)
    }
}

#[cfg(not(test))]
fn dynamic_cached_price(model_name: &str) -> Option<ModelPrice> {
    static CACHE: OnceLock<Mutex<DynamicPricingCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(DynamicPricingCache::default()));
    let Ok(mut cache) = cache.lock() else {
        return None;
    };
    let source_files = dynamic_price_cache_paths();
    let source_signature = source_files
        .iter()
        .map(|path| {
            fs::metadata(path)
                .ok()
                .and_then(|metadata| Some((metadata.modified().ok()?, metadata.len())))
        })
        .collect::<Vec<_>>();
    if cache.signature != source_signature {
        cache.entries = load_dynamic_price_cache(&source_files);
        cache.signature = source_signature;
    }
    find_dynamic_price(&cache.entries, model_name)
}

fn find_dynamic_price(
    entries: &StdHashMap<String, ModelPrice>,
    model_name: &str,
) -> Option<ModelPrice> {
    let clean = model_name.trim().to_lowercase();
    let stripped = clean
        .trim_start_matches("openrouter/")
        .trim_start_matches("openai/")
        .trim_start_matches("google/")
        .trim_start_matches("vertex_ai/")
        .trim_start_matches("x-ai/")
        .trim_start_matches("cursor-");
    entries
        .get(&clean)
        .or_else(|| entries.get(stripped))
        .or_else(|| {
            let provider = if stripped.starts_with("gpt-") {
                "openai"
            } else if stripped.starts_with("gemini-") {
                "google"
            } else if stripped.starts_with("claude-") {
                "anthropic"
            } else {
                return None;
            };
            entries.get(&format!("{provider}/{stripped}"))
        })
        .copied()
        .or_else(|| {
            entries.iter().find_map(|(key, price)| {
                (stripped.starts_with(key) || key.ends_with(&format!("/{stripped}")))
                    .then_some(*price)
            })
        })
}

#[cfg(not(test))]
#[derive(Default)]
struct DynamicPricingCache {
    signature: Vec<Option<(SystemTime, u64)>>,
    entries: StdHashMap<String, ModelPrice>,
}

#[cfg(not(test))]
fn dynamic_price_cache_paths() -> Vec<PathBuf> {
    let Some(root) = env::var_os("LOCALAPPDATA")
        .or_else(|| env::var_os("XDG_CACHE_HOME"))
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    let cache_dir = root.join("Agent Burn").join("pricing-cache");
    vec![
        cache_dir.join("litellm.json"),
        cache_dir.join("models-dev.json"),
    ]
}

#[cfg(not(test))]
fn load_dynamic_price_cache(paths: &[PathBuf]) -> StdHashMap<String, ModelPrice> {
    let mut entries = StdHashMap::new();
    if let Some(path) = paths.first() {
        if let Some(json) = read_dynamic_pricing_file(path) {
            load_litellm_cache(&json, &mut entries);
        }
    }
    if let Some(path) = paths.get(1) {
        if let Some(json) = read_dynamic_pricing_file(path) {
            load_models_dev_cache(&json, &mut entries);
        }
    }
    entries
}

#[cfg(not(test))]
fn read_dynamic_pricing_file(path: &std::path::Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > MAX_DYNAMIC_PRICING_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn load_litellm_cache(json: &str, entries: &mut StdHashMap<String, ModelPrice>) {
    let Ok(models) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json)
    else {
        return;
    };
    for (name, value) in models {
        let Some(input) = value
            .get("input_cost_per_token")
            .and_then(serde_json::Value::as_f64)
        else {
            continue;
        };
        let Some(output) = value
            .get("output_cost_per_token")
            .and_then(serde_json::Value::as_f64)
        else {
            continue;
        };
        entries.insert(
            name.to_lowercase(),
            ModelPrice {
                input_per_m: input * 1_000_000.0,
                output_per_m: output * 1_000_000.0,
                cache_read_per_m: value
                    .get("cache_read_input_token_cost")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(input * 0.1)
                    * 1_000_000.0,
                cache_write_per_m: value
                    .get("cache_creation_input_token_cost")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(input * 1.25)
                    * 1_000_000.0,
            },
        );
    }
}

fn load_models_dev_cache(json: &str, entries: &mut StdHashMap<String, ModelPrice>) {
    let Ok(providers) = serde_json::from_str::<serde_json::Value>(json) else {
        return;
    };
    let Some(providers) = providers.as_object() else {
        return;
    };
    for (provider, data) in providers {
        let Some(models) = data.get("models").and_then(serde_json::Value::as_object) else {
            continue;
        };
        for (key, model) in models {
            let Some(cost) = model.get("cost") else {
                continue;
            };
            let Some(input) = cost.get("input").and_then(serde_json::Value::as_f64) else {
                continue;
            };
            let Some(output) = cost.get("output").and_then(serde_json::Value::as_f64) else {
                continue;
            };
            let name = model
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(key)
                .to_lowercase();
            let price = ModelPrice {
                input_per_m: input,
                output_per_m: output,
                cache_read_per_m: cost
                    .get("cache_read")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(input * 0.1),
                cache_write_per_m: cost
                    .get("cache_write")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(input * 1.25),
            };
            entries.entry(name.clone()).or_insert(price);
            entries.entry(format!("{provider}/{name}")).or_insert(price);
        }
    }
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

    #[test]
    fn variant_matching_prefers_the_most_specific_model_name() {
        assert_eq!(
            get_model_pricing("gemini-2.5-flash-lite-preview-12-2026"),
            get_model_pricing("gemini-2.5-flash-lite")
        );
        assert_eq!(
            get_model_pricing("gpt-5.1-codex-mini-preview"),
            get_model_pricing("gpt-5.1-codex-mini")
        );
    }

    #[test]
    fn parses_live_litellm_rates_in_dollars_per_million() {
        let mut entries = StdHashMap::new();
        load_litellm_cache(
            r#"{"openai/gpt-6-luna":{"input_cost_per_token":0.000002,"output_cost_per_token":0.00001,"cache_read_input_token_cost":0.0000002,"cache_creation_input_token_cost":0.0000025}}"#,
            &mut entries,
        );
        let price = entries["openai/gpt-6-luna"];
        assert!((price.input_per_m - 2.0).abs() < f64::EPSILON);
        assert!((price.output_per_m - 10.0).abs() < f64::EPSILON);
        assert!((price.cache_read_per_m - 0.2).abs() < f64::EPSILON);
        assert!((price.cache_write_per_m - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn dynamically_cached_provider_prices_resolve_unqualified_model_names() {
        let entries = StdHashMap::from([(
            "openai/gpt-6-luna".to_string(),
            ModelPrice {
                input_per_m: 2.0,
                output_per_m: 10.0,
                cache_read_per_m: 0.2,
                cache_write_per_m: 2.5,
            },
        )]);
        assert_eq!(
            find_dynamic_price(&entries, "gpt-6-luna")
                .unwrap()
                .input_per_m,
            2.0
        );
        assert_eq!(
            find_dynamic_price(&entries, "openai/gpt-6-luna")
                .unwrap()
                .output_per_m,
            10.0
        );
    }

    #[test]
    fn parses_models_dev_provider_model_records() {
        let mut entries = StdHashMap::new();
        load_models_dev_cache(
            r#"{"openai":{"models":{"gpt-6-luna":{"id":"gpt-6-luna","cost":{"input":2,"output":10,"cache_read":0.2,"cache_write":2.5}}}}}"#,
            &mut entries,
        );
        assert_eq!(entries["gpt-6-luna"].input_per_m, 2.0);
        assert_eq!(entries["openai/gpt-6-luna"].output_per_m, 10.0);
    }
}
