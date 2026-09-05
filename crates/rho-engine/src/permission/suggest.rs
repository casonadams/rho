use serde_json::Value;

const INPUT_KEYS: [&str; 4] = ["command", "url", "query", "path"];

pub fn match_input(args: &Value) -> String {
    for key in INPUT_KEYS {
        if let Some(Value::String(value)) = args.get(key) {
            return value.clone();
        }
    }
    serde_json::to_string(args).unwrap_or_default()
}

pub fn suggested_rule(tool: &str, input: &str) -> String {
    match tool {
        "bash" => {
            let words: Vec<&str> = input.split_whitespace().collect();
            if words.len() >= 2
                && !words[1].starts_with('-')
                && !words[1].starts_with('/')
                && !words[1].starts_with('.')
                && !words[1].contains('/')
            {
                format!("{} {} *", words[0], words[1])
            } else if let Some(first) = words.first() {
                format!("{first} *")
            } else {
                "*".to_string()
            }
        }
        "fetch" => match input.split_once("://") {
            Some((scheme, rest)) => format!("{scheme}://{}/*", rest.split('/').next().unwrap_or_default()),
            _ => "*".to_string(),
        },
        "read" | "write" | "edit" => format!("{input}/*"),
        _ => "*".to_string(),
    }
}

pub fn canonical_tool(tool: &str) -> &str {
    match tool {
        "webfetch" | "web_fetch" => "fetch",
        "websearch" | "web_search" => "search",
        other => other,
    }
}
