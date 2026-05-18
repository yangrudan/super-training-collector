//! Jaccard 相似度计算

use std::collections::HashSet;

/// 计算两个集合的 Jaccard 相似度
///
/// Jaccard 相似度 = |A ∩ B| / |A ∪ B|
pub fn jaccard_similarity(set1: &HashSet<String>, set2: &HashSet<String>) -> f64 {
    // 如果任一集合为空，返回 0.0（无法判断相似度）
    // 这样可以避免采集失败时误判为 HANG
    if set1.is_empty() || set2.is_empty() {
        return 0.0;
    }

    let intersection = set1.intersection(set2).count() as f64;
    let union = set1.union(set2).count() as f64;

    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// 将堆栈行转换为集合，行号是否保留可选
///
/// `keep_line_numbers = false`（默认旧行为）：忽略行号，便于跨进程对齐；
/// `keep_line_numbers = true`：保留行号，分辨"函数内代码推进"的真实变化，
/// 用于区分"长 step（仍在跑）"和"真正卡死"。
pub fn stack_to_set_with_options(stack: &[String], keep_line_numbers: bool) -> HashSet<String> {
    if keep_line_numbers {
        return stack.iter().cloned().collect();
    }
    stack_to_set(stack)
}

/// 将堆栈行转换为集合，忽略行号
pub fn stack_to_set(stack: &[String]) -> HashSet<String> {
    stack
        .iter()
        .map(|line| {
            // 格式为: "function_name (filename:line_number)"
            // 需要转换为: "function_name (filename)"
            if let Some(paren_pos) = line.rfind('(') {
                if let Some(colon_pos) = line[paren_pos..].find(':') {
                    let colon_absolute_pos = paren_pos + colon_pos;
                    // 有括号和冒号，提取括号前的部分和括号内冒号前的部分
                    let before_paren = line[..paren_pos].trim_end();
                    let inside_paren = &line[paren_pos + 1..colon_absolute_pos];
                    format!("{} ({})", before_paren, inside_paren)
                } else {
                    // 有括号但没有冒号，保持原样
                    line.clone()
                }
            } else if let Some(colon_pos) = line.rfind(':') {
                // 没有括号但有冒号，检查冒号后是否为数字
                let after_colon = &line[colon_pos + 1..];
                if after_colon.chars().all(|c| c.is_numeric()) {
                    // 这可能是 function:line 的格式
                    line[..colon_pos].to_string()
                } else {
                    line.clone()
                }
            } else {
                line.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jaccard_similarity_identical() {
        let set1: HashSet<String> = vec!["a".to_string(), "b".to_string()].into_iter().collect();
        let set2 = set1.clone();

        assert_eq!(jaccard_similarity(&set1, &set2), 1.0);
    }

    #[test]
    fn test_jaccard_similarity_disjoint() {
        let set1: HashSet<String> = vec!["a".to_string(), "b".to_string()].into_iter().collect();
        let set2: HashSet<String> = vec!["c".to_string(), "d".to_string()].into_iter().collect();

        assert_eq!(jaccard_similarity(&set1, &set2), 0.0);
    }

    #[test]
    fn test_jaccard_similarity_partial() {
        let set1: HashSet<String> = vec!["a".to_string(), "b".to_string(), "c".to_string()]
            .into_iter()
            .collect();
        let set2: HashSet<String> = vec!["b".to_string(), "c".to_string(), "d".to_string()]
            .into_iter()
            .collect();

        // intersection: {b, c} = 2
        // union: {a, b, c, d} = 4
        // similarity = 2/4 = 0.5
        assert_eq!(jaccard_similarity(&set1, &set2), 0.5);
    }
}
