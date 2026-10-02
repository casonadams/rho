#!/usr/bin/env python3
# Hook for .agents/hooks/on_tool_call
# Automatically optimizes bash commands using RTK (https://github.com/rtk-ai/rtk).
import json
import os
import re
import subprocess
import sys

# Commands natively supported by RTK (built-in proxies, rules, and TOML filters)
RTK_COMMANDS = frozenset({
    # Version control & forge CLIs
    "git", "yadm", "gh", "glab", "gt", "jj",
    # Rust & Cargo
    "cargo",
    # Node / JavaScript / TypeScript package managers & runtimes
    "pnpm", "npm", "npx", "pnpx", "bun", "bunx", "deno",
    # JavaScript & TypeScript tools
    "tsc", "eslint", "biome", "prettier", "next", "jest", "vitest",
    "playwright", "prisma", "oxlint", "turbo", "nx",
    # Python tools
    "pytest", "mypy", "ruff", "sqlfluff", "pip", "pip3", "uv", "poetry",
    # Ruby tools
    "bundle", "rake", "rails", "rspec", "rubocop",
    # PHP tools
    "php", "phpunit", "phpstan", "pest", "paratest", "ecs", "pint",
    # Go tools
    "go", "golangci-lint", "golangci",
    # Build tools & compilers
    "make", "just", "task", "sbt", "gradle", "gradlew", "mvn", "mvnd", "mvnw",
    "ctest", "xcodebuild", "swift", "dotnet", "gcc", "g++",
    # Containers & Cloud & Orchestration
    "docker", "kubectl", "oc", "helm", "skopeo",
    "aws", "gcloud", "terraform", "tofu", "pulumi",
    # Database
    "psql", "liquibase",
    # File & Search utilities
    "ls", "tree", "find", "grep", "rg", "ast-grep", "diff",
    "cat", "head", "tail", "wc", "stat",
    # Network & Transfer
    "curl", "wget", "ping", "ssh", "rsync",
    # System inspection & management
    "ps", "df", "du", "systemctl", "iptables", "fail2ban-client",
    # Linters, formatters & misc tools
    "shellcheck", "yamllint", "markdownlint", "hadolint", "pre-commit",
    "ansible-playbook", "composer", "mix", "quarto", "shopify", "sops",
    "basedpyright", "ty", "mise", "ollama", "jira", "jq", "pio",
})

TRANSPARENT_PREFIXES = frozenset({
    "env", "time", "nohup", "nice", "timeout", "builtin", "command",
    "exec", "noglob", "nocorrect",
})

ENV_VAR_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=.*")
OPERATOR_SPLIT_RE = re.compile(r"(&&|\|\||[;&|\n\(\)`])")


def is_rtk_candidate(cmd):
    if not cmd or cmd.startswith("rtk "):
        return False

    segments = OPERATOR_SPLIT_RE.split(cmd)
    for seg in segments:
        seg = seg.strip()
        if not seg or seg in ("&&", "||", ";", "&", "|", "\n", "(", ")", "`"):
            continue

        tokens = seg.split()
        idx = 0
        while idx < len(tokens):
            token = tokens[idx].strip("\"'")

            # Environment variable assignments (e.g. VAR=1)
            if ENV_VAR_RE.match(token):
                idx += 1
                continue

            # Transparent wrapper prefixes (e.g. time, nohup, env)
            base = os.path.basename(token)
            if base in TRANSPARENT_PREFIXES:
                idx += 1
                while idx < len(tokens) and (tokens[idx].startswith("-") or ENV_VAR_RE.match(tokens[idx])):
                    idx += 1
                continue

            # Sudo is intentionally never rewritten by RTK
            if base == "sudo":
                break

            # Python check: only pytest, mypy, and run-tests.php are handled by RTK
            if base in ("python", "python3") or base.startswith("python3."):
                args = tokens[idx + 1:]
                if (len(args) >= 2 and args[0] == "-m" and args[1] in ("pytest", "mypy")) or any("run-tests.php" in a for a in args):
                    return True
                break

            # Java check: only Spring Boot jars are handled by RTK
            if base == "java":
                rest = " ".join(tokens[idx + 1:]).lower()
                if "spring" in rest and ".jar" in rest:
                    return True
                break

            if base in RTK_COMMANDS:
                return True
            break

    return False


def rewrite_command(cmd):
    if not is_rtk_candidate(cmd):
        return None
    try:
        res = subprocess.run(["rtk", "rewrite", cmd], capture_output=True, text=True)
    except FileNotFoundError:
        return None
    rewritten = res.stdout.strip()
    if res.returncode in (0, 3) and rewritten and rewritten != cmd:
        return rewritten
    return None


def main():
    try:
        event = json.load(sys.stdin)
        if event.get("tool_name") != "bash":
            return

        args = event.get("args") or {}
        cmd = args.get("command", "")
        rewritten = rewrite_command(cmd)
        if rewritten:
            args["command"] = rewritten
            print(json.dumps({"action": "rewrite_args", "args": args}))
    except Exception:
        pass


if __name__ == "__main__":
    main()
