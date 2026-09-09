//! # RFC 9309 Robots Exclusion Protocol Parser
//!
//! Compliant parser for `/robots.txt` supporting User-Agent prioritization,
//! longest-match precedence, wildcard patterns (`*`, `$`), and `Crawl-delay`.
//!
//! ## RFC 9309 Compliance Principles
//!
//! 1. **User-Agent Matching**:
//!    The crawler selects the most specific matching `User-agent` group (case-insensitive).
//!    If no specific token matches, the group with the wildcard `User-agent: *` is used.
//!
//! 2. **Longest Match Precedence (RFC 9309 § 2.2.2)**:
//!    When multiple rules match a URL path, the rule with the greatest character length wins:
//!    ```text
//!    Allow: /products/               (10 chars)
//!    Disallow: /products/archived/   (19 chars)
//!    -> URL /products/archived/123 is DISALLOWED
//!    ```
//!
//! 3. **Allow Overrides Disallow on Equal Length**:
//!    If an `Allow` and `Disallow` rule match with identical character length,
//!    `Allow` takes precedence.
//!
//! 4. **Wildcards & Anchors**:
//!    - `*` represents 0 or more characters.
//!    - `$` at the end anchors the pattern to the end of the URL path.
//!
//! ## Examples
//!
//! ```rust
//! use blacksparrow::crawler::robots::RobotsTxt;
//!
//! let robots = RobotsTxt::parse(r#"
//! User-agent: *
//! Disallow: /private/
//! Allow: /private/open/
//! "#);
//!
//! assert!(robots.is_allowed("BlackSparrow", "/public/index.html"));
//! assert!(!robots.is_allowed("BlackSparrow", "/private/secret.html"));
//! assert!(robots.is_allowed("BlackSparrow", "/private/open/readme.txt"));
//! ```

use std::time::Duration;

/// An individual Allow or Disallow directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotsRule {
    /// True for `Allow:`, false for `Disallow:`.
    pub is_allow: bool,
    /// Directive pattern (e.g. `/path/*`).
    pub pattern: String,
    /// Character length of the pattern for RFC 9309 precedence.
    pub length: usize,
}

/// A group of directives applying to one or more User-Agents.
#[derive(Debug, Clone, Default)]
pub struct RobotsGroup {
    /// Target User-Agents (lowercase).
    pub user_agents: Vec<String>,
    /// Ordered rules belonging to this record.
    pub rules: Vec<RobotsRule>,
    /// Optional Crawl-delay for this group.
    pub crawl_delay: Option<Duration>,
}

/// Parsed representation of a `/robots.txt` file.
#[derive(Debug, Clone, Default)]
pub struct RobotsTxt {
    /// User-agent grouped directive records.
    groups: Vec<RobotsGroup>,
    /// Discovered `Sitemap:` URLs.
    sitemaps: Vec<String>,
}

impl RobotsTxt {
    /// Parses a raw `/robots.txt` string per RFC 9309 syntax.
    ///
    /// Ignores comments (prefixed with `#`) and handles multi-agent groups.
    pub fn parse(content: &str) -> Self {
        let mut groups: Vec<RobotsGroup> = Vec::new();
        let mut current_agents: Vec<String> = Vec::new();
        let mut current_rules: Vec<RobotsRule> = Vec::new();
        let mut current_delay: Option<Duration> = None;
        let mut sitemaps: Vec<String> = Vec::new();

        for line in content.lines() {
            // Strip comments and leading/trailing whitespace
            let cleaned = line.split('#').next().unwrap_or("").trim();
            if cleaned.is_empty() {
                continue;
            }

            // Split into key and value
            let mut parts = cleaned.splitn(2, ':');
            let key = match parts.next() {
                Some(k) => k.trim().to_lowercase(),
                None => continue,
            };
            let value = match parts.next() {
                Some(v) => v.trim(),
                None => continue,
            };

            match key.as_str() {
                "user-agent" => {
                    // If we previously had rules and encounter user-agent, flush current group
                    if !current_rules.is_empty() || current_delay.is_some() {
                        if !current_agents.is_empty() {
                            groups.push(RobotsGroup {
                                user_agents: std::mem::take(&mut current_agents),
                                rules: std::mem::take(&mut current_rules),
                                crawl_delay: current_delay.take(),
                            });
                        } else {
                            current_rules.clear();
                            current_delay = None;
                        }
                    }
                    current_agents.push(value.to_lowercase());
                }
                "allow" => {
                    if !value.is_empty() {
                        let normalized_pat = ensure_leading_slash(value);
                        let length = normalized_pat.len();
                        current_rules.push(RobotsRule {
                            is_allow: true,
                            pattern: normalized_pat,
                            length,
                        });
                    }
                }
                "disallow" => {
                    if !value.is_empty() {
                        let normalized_pat = ensure_leading_slash(value);
                        let length = normalized_pat.len();
                        current_rules.push(RobotsRule {
                            is_allow: false,
                            pattern: normalized_pat,
                            length,
                        });
                    } else {
                        // Empty Disallow: resets/allows all access
                        current_rules.push(RobotsRule {
                            is_allow: true,
                            pattern: "/".to_string(),
                            length: 0,
                        });
                    }
                }
                "crawl-delay" => {
                    if let Ok(secs) = value.parse::<f64>() {
                        if secs >= 0.0 {
                            current_delay = Some(Duration::from_secs_f64(secs));
                        }
                    }
                }
                "sitemap" if !value.is_empty() => {
                    sitemaps.push(value.to_string());
                }
                _ => {}
            }
        }

        // Flush trailing group
        if !current_agents.is_empty() {
            groups.push(RobotsGroup {
                user_agents: current_agents,
                rules: current_rules,
                crawl_delay: current_delay,
            });
        }

        Self { groups, sitemaps }
    }

    /// Evaluates whether a given User-Agent is allowed to crawl the specified path or URL.
    ///
    /// Adheres strictly to RFC 9309:
    /// - Selects the most specific User-Agent group (falling back to `*`).
    /// - Finds matching rules and resolves collisions using longest-match.
    /// - If `Allow` and `Disallow` match with equal length, `Allow` wins.
    ///
    /// # Arguments
    ///
    /// * `user_agent` - Name or token of the crawler (e.g. `"BlackSparrow"`).
    /// * `path_or_url` - Relative path or full URL being checked.
    pub fn is_allowed(&self, user_agent: &str, path_or_url: &str) -> bool {
        let path = extract_path(path_or_url);
        let group = match self.find_group_for_agent(user_agent) {
            Some(g) => g,
            None => return true, // No matching group -> everything allowed
        };

        if group.rules.is_empty() {
            return true;
        }

        let mut max_matched_len = 0;
        let mut matched_is_allow: Option<bool> = None;

        for rule in &group.rules {
            if matches_pattern(&rule.pattern, path) {
                if rule.length > max_matched_len || matched_is_allow.is_none() {
                    max_matched_len = rule.length;
                    matched_is_allow = Some(rule.is_allow);
                } else if rule.length == max_matched_len && rule.is_allow {
                    // RFC 9309 § 2.2.2: Equal length tie goes to Allow
                    matched_is_allow = Some(true);
                }
            }
        }

        // Default to allowed (true) if no rules matched
        matched_is_allow.unwrap_or(true)
    }

    /// Retrieves the `Crawl-delay` directive configured for the given User-Agent.
    pub fn crawl_delay(&self, user_agent: &str) -> Option<Duration> {
        self.find_group_for_agent(user_agent)
            .and_then(|g| g.crawl_delay)
    }

    /// Returns all discovered `Sitemap:` URLs declared in the robots file.
    #[inline]
    pub fn sitemaps(&self) -> &[String] {
        &self.sitemaps
    }

    /// Selects the matching `RobotsGroup` following RFC 9309 precedence:
    /// 1. Specific agent token match (case-insensitive substring/prefix)
    /// 2. Wildcard `*` group
    fn find_group_for_agent(&self, user_agent: &str) -> Option<&RobotsGroup> {
        let ua_lower = user_agent.to_lowercase();

        // 1. Look for specific match
        for group in &self.groups {
            for agent in &group.user_agents {
                if agent != "*" && (ua_lower.contains(agent) || agent.contains(&ua_lower)) {
                    return Some(group);
                }
            }
        }

        // 2. Fall back to wildcard '*'
        for group in &self.groups {
            for agent in &group.user_agents {
                if agent == "*" {
                    return Some(group);
                }
            }
        }

        None
    }
}

/// Ensures a robots rule pattern begins with `/`.
fn ensure_leading_slash(val: &str) -> String {
    if val.starts_with('/') || val.starts_with('*') {
        val.to_string()
    } else {
        format!("/{}", val)
    }
}

/// Extracts path and query from a full URL or relative path.
fn extract_path(path_or_url: &str) -> &str {
    if let Some(pos) = path_or_url.find("://") {
        let after_scheme = &path_or_url[pos + 3..];
        if let Some(slash_pos) = after_scheme.find('/') {
            &after_scheme[slash_pos..]
        } else {
            "/"
        }
    } else if path_or_url.starts_with('/') {
        path_or_url
    } else {
        "/"
    }
}

/// Matches an RFC 9309 pattern (with `*` wildcards and `$` end anchor) against a path.
pub fn matches_pattern(pattern: &str, path: &str) -> bool {
    let (pat, anchored) = if let Some(stripped) = pattern.strip_suffix('$') {
        (stripped, true)
    } else {
        (pattern, false)
    };

    let p = pat.as_bytes();
    let s = path.as_bytes();

    let mut p_idx = 0;
    let mut s_idx = 0;
    let mut star_p: Option<usize> = None;
    let mut star_s = 0;

    while s_idx < s.len() {
        if p_idx < p.len() && p[p_idx] == b'*' {
            star_p = Some(p_idx);
            p_idx += 1;
            star_s = s_idx;
        } else if p_idx < p.len() && p[p_idx] == s[s_idx] {
            p_idx += 1;
            s_idx += 1;
            if !anchored && p_idx == p.len() {
                return true;
            }
        } else if let Some(sp) = star_p {
            p_idx = sp + 1;
            star_s += 1;
            s_idx = star_s;
        } else {
            return false;
        }
    }

    while p_idx < p.len() && p[p_idx] == b'*' {
        p_idx += 1;
    }

    if anchored {
        p_idx == p.len() && s_idx == s.len()
    } else {
        p_idx == p.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_matcher_variants() {
        assert!(matches_pattern("/products/", "/products/catalog"));
        assert!(matches_pattern(
            "/products/archived/",
            "/products/archived/item-1"
        ));
        assert!(!matches_pattern("/products/archived/", "/products/catalog"));
        assert!(matches_pattern("/*.php$", "/index.php"));
        assert!(!matches_pattern("/*.php$", "/index.php?query=1"));
        assert!(!matches_pattern("/*.php$", "/index.phps"));
        assert!(matches_pattern("/temp*preview/", "/temp-test-preview/item"));
        assert!(matches_pattern("/temp*preview/", "/temppreview/"));
        assert!(!matches_pattern("/temp*preview/", "/temp/not-match"));
    }
}
