// Bounded cross-source claim matching with inspectable heuristic agreement scores.
use super::embedder::Embedder;
use super::error::KrakenError;
use super::math::{cosine_similarity, hybrid_keyword_similarity};
use super::types::{Claim, VerificationStatus, VerifiedFact};

// Keyword overlap and optional cosine matching use explicit, fixed thresholds.
const KEYWORD_SIM_THRESHOLD: f64 = 0.5;
const SEMANTIC_SIM_THRESHOLD: f64 = 0.70;

/// Verify claims by clustering similar ones across sources and computing confidence.
pub async fn verify_claims(
    claims_by_source: &[Vec<Claim>],
    min_confidence: f64,
    embedder: Option<&dyn Embedder>,
) -> Result<(Vec<VerifiedFact>, bool), KrakenError> {
    validate_claims(claims_by_source, min_confidence)?;
    let all_claims: Vec<&Claim> = claims_by_source.iter().flat_map(|v| v.iter()).collect();
    if all_claims.is_empty() {
        return Ok((Vec::new(), false));
    }

    let total_sources = all_claims
        .iter()
        .map(|claim| source_key(&claim.source))
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    if total_sources > 20 {
        return Err(KrakenError::InvalidInput(
            "At most 20 source identities are supported".into(),
        ));
    }

    // Pre-compute embeddings if semantic mode available
    let embeddings = precompute_embeddings(&all_claims, embedder).await;
    let semantic_mode = !embeddings.is_empty();

    // Cluster similar claims
    let clusters = cluster_claims(&all_claims, &embeddings);

    // Convert clusters to verified facts
    let facts = clusters
        .into_iter()
        .map(|cluster| cluster_fact(&cluster, &all_claims, total_sources, min_confidence))
        .collect();
    Ok((facts, semantic_mode))
}

fn cluster_fact(
    cluster: &[usize],
    all_claims: &[&Claim],
    total_sources: usize,
    min_confidence: f64,
) -> VerifiedFact {
    let unique_sources: std::collections::BTreeSet<String> = cluster
        .iter()
        .map(|&i| source_key(&all_claims[i].source))
        .collect();
    let agree_count = unique_sources.len();
    let confidence = calculate_confidence(agree_count, total_sources);
    let representative = select_representative(cluster, all_claims);
    let contradiction = detect_contradiction(cluster, all_claims);
    let status = if contradiction.is_some() {
        VerificationStatus::Flagged
    } else {
        determine_status(confidence, min_confidence)
    };

    VerifiedFact {
        claim: all_claims[representative].text.clone(),
        confidence,
        sources: unique_sources.into_iter().collect(),
        status,
        contradiction,
    }
}

fn source_key(source: &str) -> String {
    if let Ok(mut url) = url::Url::parse(source) {
        url.set_fragment(None);
        return url.to_string();
    }
    source.to_owned()
}

fn validate_claims(groups: &[Vec<Claim>], threshold: f64) -> Result<(), KrakenError> {
    let invalid = groups.len() > 20
        || !threshold.is_finite()
        || !(0.5..=1.0).contains(&threshold)
        || groups.iter().any(|group| {
            group.len() > 15
                || group.iter().any(|claim| {
                    claim.text.trim().is_empty()
                        || claim.text.len() > 1500
                        || claim.text.contains('\0')
                        || claim.source.trim().is_empty()
                        || claim.source.len() > 8192
                        || claim.source.chars().any(char::is_control)
                        || claim.context.len() > 4096
                        || claim.keywords.len() > 64
                        || claim.keywords.iter().any(|word| word.len() > 256)
                })
        });
    if invalid {
        return Err(KrakenError::InvalidInput(
            "Claim verification input exceeds supported limits".into(),
        ));
    }
    Ok(())
}

async fn precompute_embeddings(
    claims: &[&Claim],
    embedder: Option<&dyn Embedder>,
) -> Vec<Vec<f32>> {
    let Some(emb) = embedder else {
        return Vec::new();
    };
    if !emb.is_available() {
        return Vec::new();
    }

    let texts: Vec<String> = claims.iter().map(|c| c.text.clone()).collect();
    let Ok(Ok(vectors)) =
        tokio::time::timeout(std::time::Duration::from_secs(10), emb.embed_batch(&texts)).await
    else {
        return Vec::new();
    };
    let dimension = vectors.first().map_or(0, Vec::len);
    if vectors.len() != claims.len()
        || dimension == 0
        || dimension > 8192
        || (claims.len() * claims.len().saturating_sub(1) / 2).saturating_mul(dimension)
            > 50_000_000
        || vectors.iter().any(|v| {
            v.len() != dimension || v.iter().any(|n| !n.is_finite()) || v.iter().all(|n| *n == 0.0)
        })
    {
        return Vec::new();
    }
    vectors
}

/// Group claims into clusters based on keyword/semantic similarity.
fn cluster_claims(claims: &[&Claim], embeddings: &[Vec<f32>]) -> Vec<Vec<usize>> {
    let n = claims.len();
    let mut assigned = vec![false; n];
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let has_embeddings = embeddings.len() == n;

    for i in 0..n {
        if assigned[i] {
            continue;
        }
        let mut cluster = vec![i];
        assigned[i] = true;

        for j in (i + 1)..n {
            if assigned[j] {
                continue;
            }

            // Fast path: hybrid keyword similarity (max of Jaccard / Overlap)
            let kw_sim = hybrid_keyword_similarity(&claims[i].keywords, &claims[j].keywords);
            if kw_sim >= KEYWORD_SIM_THRESHOLD {
                cluster.push(j);
                assigned[j] = true;
                continue;
            }

            // Slow path: semantic similarity (if embeddings available)
            if has_embeddings {
                let sem_sim = cosine_similarity(&embeddings[i], &embeddings[j]);
                if sem_sim >= SEMANTIC_SIM_THRESHOLD {
                    cluster.push(j);
                    assigned[j] = true;
                }
            }
        }

        clusters.push(cluster);
    }

    clusters
}

/// Each accepted cluster requires at least three distinct source identities.
const MIN_SOURCES_FOR_VERIFICATION: usize = 3;

fn calculate_confidence(agree_count: usize, total_sources: usize) -> f64 {
    if total_sources == 0 || agree_count == 0 {
        return 0.0;
    }

    let raw_confidence = agree_count as f64 / total_sources as f64;

    // Below the minimum, cap so confidence can never cross the verified line.
    if total_sources < MIN_SOURCES_FOR_VERIFICATION || agree_count < MIN_SOURCES_FOR_VERIFICATION {
        return raw_confidence.min(0.39);
    }

    raw_confidence
}

fn determine_status(confidence: f64, min_confidence: f64) -> VerificationStatus {
    if confidence >= min_confidence {
        VerificationStatus::Verified
    } else if confidence >= 0.4 {
        VerificationStatus::Pending
    } else {
        VerificationStatus::Unverified
    }
}

/// Pick the best representative: longest text with most keywords.
fn select_representative(cluster: &[usize], claims: &[&Claim]) -> usize {
    *cluster
        .iter()
        .max_by_key(|&&i| claims[i].text.len() + claims[i].keywords.len() * 10)
        .unwrap_or(&cluster[0])
}

/// Word-boundary aware negation check.
/// ASCII words ("not", "no", ...) match as whole tokens so "nothing"/"notes"
/// don't trigger. Korean negation stems (U+C5C6, U+C54A) remain substring
/// matches since they attach to verb/adjective roots as eomi suffixes.
fn contains_negation(text_lower: &str) -> bool {
    const NEGATIONS: &[&str] = &[
        "not", "no", "never", "don't", "doesn't", "cannot", "없", "않",
    ];
    NEGATIONS
        .iter()
        .any(|kw| crate::text::contains_keyword_bounded(text_lower, kw))
}

/// Detect contradiction within a cluster by checking for negation words.
fn detect_contradiction(cluster: &[usize], claims: &[&Claim]) -> Option<String> {
    let mut has_positive = false;
    let mut has_negative = false;
    let mut neg_text = String::new();

    for &i in cluster {
        let lower = claims[i].text.to_lowercase();
        if contains_negation(&lower) {
            has_negative = true;
            neg_text = claims[i].text.clone();
        } else {
            has_positive = true;
        }
    }

    if has_positive && has_negative {
        Some(format!("Contradicting claim: {}", neg_text))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cluster_requires_three_distinct_agreeing_sources() {
        let claim = |source: &str| {
            mk_claim(
                "Rust provides memory safety",
                source,
                &["rust", "memory", "safety"],
            )
        };
        let two = vec![
            vec![claim("https://a.example/page")],
            vec![claim("https://b.example/page")],
            vec![mk_claim(
                "Unrelated database concept",
                "https://c.example/page",
                &["unrelated", "database", "concept"],
            )],
        ];
        let (facts, _) = futures::executor::block_on(verify_claims(&two, 0.6, None)).unwrap();
        assert!(
            facts
                .iter()
                .all(|fact| fact.status != VerificationStatus::Verified)
        );
        let duplicated = vec![
            vec![claim("https://a.example/page")],
            vec![claim("https://a.example/page#heading")],
            vec![claim("https://b.example/page")],
        ];
        let (facts, _) =
            futures::executor::block_on(verify_claims(&duplicated, 0.6, None)).unwrap();
        assert!(
            facts
                .iter()
                .all(|fact| fact.status != VerificationStatus::Verified)
        );
        let mut three = duplicated;
        three.push(vec![claim("https://c.example/page")]);
        let (facts, _) = futures::executor::block_on(verify_claims(&three, 0.6, None)).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].status, VerificationStatus::Verified);
        assert_eq!(facts[0].confidence, 1.0);
        assert_eq!(
            facts[0].sources,
            [
                "https://a.example/page",
                "https://b.example/page",
                "https://c.example/page"
            ]
        );
    }

    #[test]
    fn invalid_thresholds_and_claim_budgets_fail_before_matching() {
        for threshold in [f64::NAN, f64::INFINITY, 0.49, 1.1] {
            assert!(futures::executor::block_on(verify_claims(&[], threshold, None)).is_err());
        }
        assert!(futures::executor::block_on(verify_claims(&vec![vec![]; 21], 0.6, None)).is_err());
        let large = vec![vec![mk_claim("Claim", "source", &["word"]); 16]];
        assert!(futures::executor::block_on(verify_claims(&large, 0.6, None)).is_err());
    }

    fn mk_claim(text: &str, source: &str, keywords: &[&str]) -> Claim {
        Claim {
            text: text.to_string(),
            source: source.to_string(),
            context: String::new(),
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn test_cluster_merges_paraphrased_claims_across_sources() {
        // Regression: three sources phrase the same fact differently. Under
        // the old Jaccard-only threshold they stayed as singletons (agree=1)
        // and every fact ended up 'unverified'. With hybrid overlap they
        // must merge into one cluster with agree_count=3.
        let claims_by_source = paraphrased_sources();

        let (facts, _) =
            futures::executor::block_on(verify_claims(&claims_by_source, 0.6, None)).unwrap();

        // At least one fact must have 3 distinct sources (the Rust memory-safety cluster)
        let best = facts.iter().map(|f| f.sources.len()).max().unwrap_or(0);
        assert!(
            best >= 3,
            "Expected paraphrased Rust-memory-safety claims to cluster across 3+ sources, got max {}",
            best,
        );
    }

    fn paraphrased_sources() -> Vec<Vec<Claim>> {
        let source_a = vec![mk_claim(
            "Rust guarantees memory safety without a garbage collector",
            "source_a",
            &[
                "rust",
                "guarantees",
                "memory",
                "safety",
                "without",
                "garbage",
                "collector",
            ],
        )];
        let source_b = vec![mk_claim(
            "Memory safety in Rust is enforced at compile time",
            "source_b",
            &["memory", "safety", "rust", "enforced", "compile", "time"],
        )];
        let source_c = vec![mk_claim(
            "Rust provides memory safety",
            "source_c",
            &["rust", "memory", "safety"],
        )];

        // 5 sources total so confidence is uncapped; two extras are unrelated.
        let source_d = vec![mk_claim(
            "unrelated topic one",
            "source_d",
            &["unrelated", "topic", "one"],
        )];
        let source_e = vec![mk_claim(
            "unrelated topic two",
            "source_e",
            &["unrelated", "topic", "two"],
        )];

        vec![source_a, source_b, source_c, source_d, source_e]
    }

    #[test]
    fn test_confidence_calculation() {
        // Three or more agreeing sources use their share of all distinct evidence sources.
        assert!((calculate_confidence(3, 3) - 1.0).abs() < f64::EPSILON);
        assert!(calculate_confidence(2, 3) <= 0.39);
        assert!((calculate_confidence(3, 5) - 0.6).abs() < f64::EPSILON);
        assert!((calculate_confidence(5, 5) - 1.0).abs() < f64::EPSILON);
        assert!((calculate_confidence(4, 8) - 0.5).abs() < f64::EPSILON);

        // < 3 sources: capped at 0.39 (cannot reach verified)
        assert!(calculate_confidence(2, 2) <= 0.39);
        assert!(calculate_confidence(1, 1) <= 0.39);
        assert!(calculate_confidence(1, 2) <= 0.39);

        // edge cases
        assert!((calculate_confidence(0, 5)).abs() < f64::EPSILON);
        assert!((calculate_confidence(0, 0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_three_source_cascade_can_verify() {
        // Three distinct agreeing sources can satisfy the configured threshold.
        let conf = calculate_confidence(3, 3);
        assert_eq!(determine_status(conf, 0.6), VerificationStatus::Verified);
    }

    #[test]
    fn test_status_determination() {
        assert_eq!(determine_status(0.8, 0.6), VerificationStatus::Verified);
        assert_eq!(determine_status(0.5, 0.6), VerificationStatus::Pending);
        assert_eq!(determine_status(0.2, 0.6), VerificationStatus::Unverified);
    }

    #[test]
    fn test_negation_word_boundary() {
        // Positive: real negations must be detected
        assert!(contains_negation("rust does not have garbage collection"));
        assert!(contains_negation("this is never true"));
        assert!(contains_negation("it doesn't compile"));
        assert!(contains_negation("you cannot borrow twice"));
        assert!(contains_negation("그런 기능은 없다"));
        assert!(contains_negation("컴파일되지 않는다"));

        // Regression: substrings must NOT trigger
        assert!(!contains_negation("nothing is wrong here"));
        assert!(!contains_negation("take note of the pattern"));
        assert!(!contains_negation("the northern region"));
        assert!(!contains_negation("notorious for safety bugs"));
    }

    #[test]
    fn test_contradiction_ignores_substring_false_positives() {
        let claims = [
            mk_claim(
                "Rust has strong memory safety",
                "src_a",
                &["rust", "memory", "safety"],
            ),
            mk_claim(
                "Nothing beats Rust for safety",
                "src_b",
                &["nothing", "beats", "rust", "safety"],
            ),
            mk_claim(
                "Take note: Rust enforces safety at compile time",
                "src_c",
                &["take", "note", "rust", "safety"],
            ),
        ];
        let cluster: Vec<usize> = (0..claims.len()).collect();
        let claim_refs: Vec<&Claim> = claims.iter().collect();
        let contradiction = detect_contradiction(&cluster, &claim_refs);
        assert!(
            contradiction.is_none(),
            "'nothing'/'note' are not negations: {:?}",
            contradiction,
        );
    }
}
