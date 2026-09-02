# Quota Tracker Plugin for `rho`

A live status-line plugin in Node.js demonstrating how provider plugins (like Google Antigravity or ChatGPT) can surface 5h and 7d rolling quota cooldowns in `rho`'s interactive footer.

## Configuration in `config.toml`

```toml
[plugins.quota_tracker]
enabled = true
command = "node"
args = ["/path/to/rho/examples/plugins/quota-tracker/tracker.js"]
```

## Live Footer Appearance

```text
~/src/github.com/casonadams/rho (main)   5h: 50% • 7d: 17%
↑4.2k ↓210 3.2%/128k @24.1t/s   claude-3-7-sonnet • high
```
