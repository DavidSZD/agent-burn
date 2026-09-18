use crate::{
    cli::CostMode,
    pricing::PricingMap,
    types::{Speed, UsageEntry},
};

const CACHE_CREATE_1H_INPUT_MULTIPLIER: f64 = 2.0;

pub(crate) fn calculate_cost(
    data: &UsageEntry,
    mode: CostMode,
    pricing: Option<&PricingMap>,
) -> f64 {
    calculate_cost_for_usage_at(
        data.message.model.as_deref(),
        data.message.usage,
        data.cost_usd,
        parse_timestamp_ms(&data.timestamp),
        mode,
        pricing,
    )
}

pub(crate) fn calculate_cost_for_usage(
    model: Option<&str>,
    usage: crate::TokenUsageRaw,
    cost_usd: Option<f64>,
    mode: CostMode,
    pricing: Option<&PricingMap>,
) -> f64 {
    calculate_cost_for_usage_at(model, usage, cost_usd, None, mode, pricing)
}

pub(crate) fn calculate_cost_for_usage_at(
    model: Option<&str>,
    usage: crate::TokenUsageRaw,
    cost_usd: Option<f64>,
    timestamp_ms: Option<i64>,
    mode: CostMode,
    pricing: Option<&PricingMap>,
) -> f64 {
    match mode {
        CostMode::Display => cost_usd.unwrap_or(0.0),
        CostMode::Auto => cost_usd
            .unwrap_or_else(|| calculate_cost_from_tokens_at(model, usage, timestamp_ms, pricing)),
        CostMode::Calculate => calculate_cost_from_tokens_at(model, usage, timestamp_ms, pricing),
    }
}

pub(crate) fn missing_pricing_model_for_usage(
    model: Option<&str>,
    usage: crate::TokenUsageRaw,
    cost_usd: Option<f64>,
    mode: CostMode,
    pricing: Option<&PricingMap>,
) -> Option<String> {
    if mode == CostMode::Display || (mode == CostMode::Auto && cost_usd.is_some()) {
        return None;
    }
    missing_pricing_model_for_token_total(model, crate::total_usage_tokens(usage), pricing)
}

pub(crate) fn missing_pricing_model_for_token_total(
    model: Option<&str>,
    total_tokens: u64,
    pricing: Option<&PricingMap>,
) -> Option<String> {
    if total_tokens == 0 {
        return None;
    }
    let model = model?;
    let pricing = pricing?;
    pricing.find(model).is_none().then(|| model.to_string())
}

pub(crate) fn missing_pricing_model_for_candidates(
    model: &str,
    candidates: impl IntoIterator<Item = String>,
    total_tokens: u64,
    pricing: Option<&PricingMap>,
) -> Option<String> {
    if total_tokens == 0 {
        return None;
    }
    let pricing = pricing?;
    candidates
        .into_iter()
        .all(|candidate| pricing.find(&candidate).is_none())
        .then(|| model.to_string())
}

fn calculate_cost_from_tokens(
    model: Option<&str>,
    usage: crate::TokenUsageRaw,
    pricing: Option<&PricingMap>,
) -> f64 {
    calculate_cost_from_tokens_at(model, usage, None, pricing)
}

fn calculate_cost_from_tokens_at(
    model: Option<&str>,
    usage: crate::TokenUsageRaw,
    timestamp_ms: Option<i64>,
    pricing: Option<&PricingMap>,
) -> f64 {
    let Some(model) = model else {
        return 0.0;
    };
    let Some(pricing_map) = pricing else {
        return 0.0;
    };
    let Some(pricing) = pricing_map.find_at(model, timestamp_ms) else {
        return 0.0;
    };
    calculate_cost_from_pricing_with_threshold(
        usage,
        pricing,
        pricing_map.long_context_threshold(model),
    )
}

fn parse_timestamp_ms(timestamp: &str) -> Option<i64> {
    crate::parse_ts_timestamp(timestamp).map(crate::TimestampMs::as_millis)
}

pub(crate) fn calculate_cost_from_pricing(
    usage: crate::TokenUsageRaw,
    pricing: crate::pricing::Pricing,
) -> f64 {
    calculate_cost_from_pricing_with_threshold(usage, pricing, 200_000)
}

pub(crate) fn calculate_cost_from_pricing_with_threshold(
    usage: crate::TokenUsageRaw,
    pricing: crate::pricing::Pricing,
    threshold: u64,
) -> f64 {
    let multiplier = if matches!(usage.speed, Some(Speed::Fast)) {
        pricing.fast_multiplier
    } else {
        1.0
    };
    let (cache_create_5m_tokens, cache_create_1h_tokens) =
        if let Some(breakdown) = usage.cache_creation {
            (
                breakdown.ephemeral_5m_input_tokens,
                breakdown.ephemeral_1h_input_tokens,
            )
        } else {
            (usage.cache_creation_input_tokens, 0)
        };
    let cache_create_1h_cost = pricing.input * CACHE_CREATE_1H_INPUT_MULTIPLIER;
    let cache_create_1h_cost_above_200k = pricing
        .input_above_200k
        .map(|c| c * CACHE_CREATE_1H_INPUT_MULTIPLIER);
    (tiered_cost_with_threshold(
        usage.input_tokens,
        pricing.input,
        pricing.input_above_200k,
        threshold,
    ) + tiered_cost_with_threshold(
        usage.output_tokens,
        pricing.output,
        pricing.output_above_200k,
        threshold,
    ) + tiered_cost_with_threshold(
        cache_create_5m_tokens,
        pricing.cache_create,
        pricing.cache_create_above_200k,
        threshold,
    ) + tiered_cost_with_threshold(
        cache_create_1h_tokens,
        cache_create_1h_cost,
        cache_create_1h_cost_above_200k,
        threshold,
    ) + tiered_cost_with_threshold(
        usage.cache_read_input_tokens,
        pricing.cache_read,
        pricing.cache_read_above_200k,
        threshold,
    )) * multiplier
}

pub(crate) fn tiered_cost(tokens: u64, base: f64, above: Option<f64>) -> f64 {
    tiered_cost_with_threshold(tokens, base, above, 200_000)
}

pub(crate) fn tiered_cost_with_threshold(
    tokens: u64,
    base: f64,
    above: Option<f64>,
    threshold: u64,
) -> f64 {
    if tokens == 0 {
        return 0.0;
    }
    if let Some(above) = above
        && tokens > threshold
    {
        return (threshold as f64 * base) + ((tokens - threshold) as f64 * above);
    }
    tokens as f64 * base
}

#[cfg(test)]
mod tests {
    use crate::{
        cli::CostMode,
        pricing::PricingMap,
        types::{CacheCreationRaw, TokenUsageRaw},
    };

    use super::calculate_cost_for_usage;

    fn pricing() -> PricingMap {
        let mut pricing = PricingMap::default();
        pricing.load_json(
            r#"{
                "test-model": {
                    "input_cost_per_token": 1.0,
                    "output_cost_per_token": 10.0,
                    "cache_creation_input_token_cost": 1.25,
                    "cache_read_input_token_cost": 0.1,
                    "input_cost_per_token_above_200k_tokens": 2.0,
                    "cache_creation_input_token_cost_above_200k_tokens": 1.5
                }
            }"#,
        );
        pricing
    }

    #[test]
    fn prices_cache_creation_breakdown_by_duration() {
        let usage = TokenUsageRaw {
            cache_creation_input_tokens: 999,
            cache_read_input_tokens: 30,
            cache_creation: Some(CacheCreationRaw {
                ephemeral_5m_input_tokens: 10,
                ephemeral_1h_input_tokens: 20,
            }),
            ..TokenUsageRaw::default()
        };

        let cost = calculate_cost_for_usage(
            Some("test-model"),
            usage,
            None,
            CostMode::Calculate,
            Some(&pricing()),
        );

        assert!((cost - 55.5).abs() < f64::EPSILON);
    }

    #[test]
    fn falls_back_to_flat_cache_creation_rate_without_breakdown() {
        let usage = TokenUsageRaw {
            cache_creation_input_tokens: 10,
            ..TokenUsageRaw::default()
        };

        let cost = calculate_cost_for_usage(
            Some("test-model"),
            usage,
            None,
            CostMode::Calculate,
            Some(&pricing()),
        );

        assert!((cost - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_cache_creation_breakdown_from_usage_json() {
        let usage = serde_json::from_str::<TokenUsageRaw>(
            r#"{
                "input_tokens": 1,
                "output_tokens": 2,
                "cache_creation_input_tokens": 300,
                "cache_creation": {
                    "ephemeral_5m_input_tokens": 100,
                    "ephemeral_1h_input_tokens": 200
                }
            }"#,
        )
        .unwrap();

        assert_eq!(usage.cache_creation_token_count(), 300);
    }

    #[test]
    fn applies_model_specific_long_context_threshold() {
        let mut pricing = PricingMap::default();
        pricing.load_json(
            r#"{
                "long-model": {
                    "input_cost_per_token": 1.0,
                    "output_cost_per_token": 1.0,
                    "input_cost_per_token_above_200k_tokens": 3.0,
                    "long_context_threshold": 272000
                }
            }"#,
        );
        let usage = TokenUsageRaw {
            input_tokens: 300_000,
            ..TokenUsageRaw::default()
        };
        let cost = calculate_cost_for_usage(
            Some("long-model"),
            usage,
            None,
            CostMode::Calculate,
            Some(&pricing),
        );
        assert!((cost - 356_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn uses_the_base_rate_just_before_a_model_specific_threshold() {
        let mut pricing = PricingMap::default();
        pricing.load_json(
            r#"{
                "long-model": {
                    "input_cost_per_token": 1.0,
                    "output_cost_per_token": 1.0,
                    "input_cost_per_token_above_200k_tokens": 3.0,
                    "long_context_threshold": 272000
                }
            }"#,
        );

        let cost = calculate_cost_for_usage(
            Some("long-model"),
            TokenUsageRaw {
                input_tokens: 271_999,
                ..TokenUsageRaw::default()
            },
            None,
            CostMode::Calculate,
            Some(&pricing),
        );

        assert!((cost - 271_999.0).abs() < f64::EPSILON);
    }

    #[test]
    fn keeps_the_base_rate_at_a_model_specific_threshold() {
        let mut pricing = PricingMap::default();
        pricing.load_json(
            r#"{
                "long-model": {
                    "input_cost_per_token": 1.0,
                    "output_cost_per_token": 1.0,
                    "input_cost_per_token_above_200k_tokens": 3.0,
                    "long_context_threshold": 272000
                }
            }"#,
        );

        let cost = calculate_cost_for_usage(
            Some("long-model"),
            TokenUsageRaw {
                input_tokens: 272_000,
                ..TokenUsageRaw::default()
            },
            None,
            CostMode::Calculate,
            Some(&pricing),
        );

        assert!((cost - 272_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn applies_the_long_context_rate_only_after_a_model_specific_threshold() {
        let mut pricing = PricingMap::default();
        pricing.load_json(
            r#"{
                "long-model": {
                    "input_cost_per_token": 1.0,
                    "output_cost_per_token": 1.0,
                    "input_cost_per_token_above_200k_tokens": 3.0,
                    "long_context_threshold": 272000
                }
            }"#,
        );

        let cost = calculate_cost_for_usage(
            Some("long-model"),
            TokenUsageRaw {
                input_tokens: 272_001,
                ..TokenUsageRaw::default()
            },
            None,
            CostMode::Calculate,
            Some(&pricing),
        );

        assert!((cost - 272_003.0).abs() < f64::EPSILON);
    }
}
