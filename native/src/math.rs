// n2-kraken: similarity and math utilities

/// Jaccard similarity between two keyword sets.
pub fn keyword_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let set_a: std::collections::HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let set_b: std::collections::HashSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let intersection = set_a.intersection(&set_b).count() as f64;
    let union = set_a.union(&set_b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// Overlap coefficient between two keyword sets: |A intersect B| / min(|A|,|B|).
/// More tolerant than Jaccard when claim keyword counts differ (common when
/// one source phrases the same fact more tersely than another).
pub fn overlap_coefficient(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let set_a: std::collections::HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let set_b: std::collections::HashSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let intersection = set_a.intersection(&set_b).count() as f64;
    let min_size = set_a.len().min(set_b.len()) as f64;
    if min_size == 0.0 {
        0.0
    } else {
        intersection / min_size
    }
}

/// Hybrid keyword similarity: max(Jaccard, Overlap).
/// Jaccard penalises asymmetric sets; overlap forgives them. Taking the max
/// keeps the stricter metric when both sides are rich and relaxes when one
/// claim is short.
pub fn hybrid_keyword_similarity(a: &[String], b: &[String]) -> f64 {
    let jac = keyword_similarity(a, b);
    let ovl = overlap_coefficient(a, b);
    jac.max(ovl)
}

/// Cosine similarity between two f32 vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut mag_a = 0.0f64;
    let mut mag_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        mag_a += x * x;
        mag_b += y * y;
    }
    let denom = mag_a.sqrt() * mag_b.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_identical() {
        let a = vec!["rust".into(), "async".into()];
        assert!((keyword_similarity(&a, &a) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_disjoint() {
        let a = vec!["rust".into()];
        let b = vec!["python".into()];
        assert!((keyword_similarity(&a, &b)).abs() < f64::EPSILON);
    }

    #[test]
    fn cosine_identical() {
        let a = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn overlap_subset_is_one() {
        let a = vec!["rust".into(), "async".into()];
        let b = vec![
            "rust".into(),
            "async".into(),
            "tokio".into(),
            "runtime".into(),
        ];
        // 2 shared / min(2,4)=2 => 1.0 (subset)
        assert!((overlap_coefficient(&a, &b) - 1.0).abs() < f64::EPSILON);
        // Jaccard for same pair: 2/4 = 0.5
        assert!((keyword_similarity(&a, &b) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn hybrid_prefers_more_generous_metric() {
        let a = vec!["rust".into(), "async".into(), "tokio".into()];
        let b = vec![
            "rust".into(),
            "async".into(),
            "runtime".into(),
            "futures".into(),
            "io".into(),
            "poll".into(),
        ];
        // Jaccard = 2/7 ~= 0.286 ; Overlap = 2/3 ~= 0.667
        assert!(hybrid_keyword_similarity(&a, &b) > 0.6);
    }

    #[test]
    fn overlap_empty_is_zero() {
        let a: Vec<String> = vec![];
        let b = vec!["rust".into()];
        assert_eq!(overlap_coefficient(&a, &b), 0.0);
    }
}
