use url::Url;

pub fn normalize_domain(raw: &str) -> Option<String> {
    let mut input = raw.trim().to_lowercase();
    if input.is_empty() {
        return None;
    }
    if let Some(stripped) = input.strip_prefix('-') {
        input = stripped.trim().to_string();
    }
    if input.is_empty() {
        return None;
    }
    if let Ok(parsed) = Url::parse(&input) {
        if let Some(host) = parsed.host_str() {
            input = host.to_string();
        }
    } else if let Ok(parsed) = Url::parse(&format!("https://{input}")) {
        if let Some(host) = parsed.host_str() {
            input = host.to_string();
        }
    } else {
        input = input.split('/').next()?.split(':').next()?.to_string();
    }
    let trimmed = input.trim_start_matches("www.").trim_matches('.').to_string();
    if trimmed.contains('.') && !trimmed.contains(' ') {
        Some(trimmed)
    } else {
        None
    }
}

pub fn normalize_domain_filters(domains: Option<&[String]>) -> (Vec<String>, Vec<String>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    let Some(domains) = domains else {
        return (allowed, blocked);
    };

    for raw in domains {
        let is_blocked = raw.trim().starts_with('-');
        if let Some(domain) = normalize_domain(raw) {
            if is_blocked {
                if !blocked.contains(&domain) {
                    blocked.push(domain);
                }
            } else if !allowed.contains(&domain) {
                allowed.push(domain);
            }
        }
    }
    (allowed, blocked)
}

pub fn matches_site(host: &str, target_domain: &str) -> bool {
    let normalized_host = host.strip_prefix("www.").unwrap_or(host).to_lowercase();
    let normalized_target = target_domain
        .strip_prefix("www.")
        .unwrap_or(target_domain)
        .to_lowercase();
    normalized_host == normalized_target || normalized_host.ends_with(&format!(".{normalized_target}"))
}

pub fn matches_domain_filters(host: &str, allowed: &[String], blocked: &[String]) -> bool {
    if allowed.is_empty() && blocked.is_empty() {
        return true;
    }
    if !allowed.is_empty() && !allowed.iter().any(|domain| matches_site(host, domain)) {
        return false;
    }
    !blocked.iter().any(|domain| matches_site(host, domain))
}

pub fn build_search_query_with_filters(query: &str, domains: Option<&[String]>) -> String {
    let cleaned = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let (allowed, blocked) = normalize_domain_filters(domains);
    if allowed.is_empty() && blocked.is_empty() {
        return cleaned;
    }

    let mut parts = vec![cleaned];
    if allowed.len() == 1 && !parts[0].to_lowercase().contains("site:") {
        parts.push(format!("site:{}", allowed[0]));
    } else if allowed.len() > 1 && !parts[0].to_lowercase().contains("site:") {
        let sites = allowed
            .iter()
            .map(|d| format!("site:{d}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        parts.push(sites);
    }

    for b in blocked {
        let neg = format!("-site:{b}");
        if !parts[0].contains(&neg) {
            parts.push(neg);
        }
    }

    parts.join(" ").trim().to_string()
}

pub fn urlencoding_encode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

pub fn relax_query(query: &str) -> String {
    query
        .replace(['"', '\'', '(', ')', '[', ']', '+'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
