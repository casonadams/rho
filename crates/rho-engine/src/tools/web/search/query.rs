use url::Url;

fn parse_host_str(input: &str) -> Option<String> {
    if let Ok(parsed) = Url::parse(input)
        && let Some(host) = parsed.host_str()
    {
        return Some(host.to_string());
    }
    if let Ok(parsed) = Url::parse(&format!("https://{input}"))
        && let Some(host) = parsed.host_str()
    {
        return Some(host.to_string());
    }
    let part = input.split('/').next()?.split(':').next()?;
    Some(part.to_string())
}

pub fn normalize_domain(raw: &str) -> Option<String> {
    let input = raw.trim().trim_start_matches('-').trim().to_lowercase();
    if input.is_empty() {
        return None;
    }
    let host = parse_host_str(&input)?;
    let trimmed = host.trim_start_matches("www.").trim_matches('.').to_string();
    (trimmed.contains('.') && !trimmed.contains(' ')).then_some(trimmed)
}

fn insert_domain_filter(raw: &str, (allowed, blocked): (&mut Vec<String>, &mut Vec<String>)) {
    let Some(domain) = normalize_domain(raw) else {
        return;
    };
    if raw.trim().starts_with('-') {
        if !blocked.contains(&domain) {
            blocked.push(domain);
        }
    } else if !allowed.contains(&domain) {
        allowed.push(domain);
    }
}

pub fn normalize_domain_filters(domains: Option<&[String]>) -> (Vec<String>, Vec<String>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    if let Some(list) = domains {
        for raw in list {
            insert_domain_filter(raw, (&mut allowed, &mut blocked));
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

fn append_allowed_sites(parts: &mut Vec<String>, allowed: &[String]) {
    if parts[0].to_lowercase().contains("site:") {
        return;
    }
    if allowed.len() == 1 {
        parts.push(format!("site:{}", allowed[0]));
    } else if allowed.len() > 1 {
        let sites = allowed
            .iter()
            .map(|d| format!("site:{d}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        parts.push(sites);
    }
}

fn append_blocked_sites(parts: &mut Vec<String>, blocked: &[String]) {
    for b in blocked {
        let neg = format!("-site:{b}");
        if !parts[0].contains(&neg) {
            parts.push(neg);
        }
    }
}

pub fn build_search_query_with_filters(query: &str, domains: Option<&[String]>) -> String {
    let cleaned = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let (allowed, blocked) = normalize_domain_filters(domains);
    if allowed.is_empty() && blocked.is_empty() {
        return cleaned;
    }

    let mut parts = vec![cleaned];
    append_allowed_sites(&mut parts, &allowed);
    append_blocked_sites(&mut parts, &blocked);
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
