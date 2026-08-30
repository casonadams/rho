---
name: create-plugin
description: Create, test, and package a capability plugin for rho. Use when asked to write a plugin, capability, tool replacement, or provider for rho.
argument-hint: "<plugin-idea-or-specification>"
---

# Creating a Capability Plugin for `rho`

`rho` plugins are standalone executables communicating over a versioned standard input/output protocol using `rho-sdk`.

## 1. Quick Template

A `rho` capability plugin defines capabilities and responds to standard JSON protocol requests:

```rust
use rho_sdk::capability::{
    CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityId, CapabilityManifest, PLUGIN_PROTOCOL_VERSION,
};
use rho_sdk::contract::{
    ExecutionMode, OperationEffect, PathScope, ToolDescriptor, ToolInvocationRequest, ToolInvocationResponse,
};
use rho_sdk::protocol::{
    Envelope, ErrorCode, InvocationRequest, ProtocolMessage, RequestId, StreamEvent, StructuredError, TerminalResult,
    decode_line, encode_line,
};
use std::io::{BufRead, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        let request = match decode_line(line.as_bytes()) {
            Ok(request) => request,
            Err(_) => continue,
        };

        match request.message {
            ProtocolMessage::HandshakeRequest { supported_versions } => {
                let envelope = Envelope::new(
                    request.request_id,
                    ProtocolMessage::TerminalResponse {
                        result: TerminalResult::Handshake { selected_version: 1 },
                    },
                );
                stdout.write_all(&encode_line(&envelope)?)?;
            }
            ProtocolMessage::DiscoveryRequest => {
                let manifest = CapabilityManifest {
                    plugin_id: "rho-plugin-mytool".parse()?,
                    plugin_version: "1.0.0".to_string(),
                    api_version: CAPABILITY_API_VERSION,
                    protocol_version: PLUGIN_PROTOCOL_VERSION,
                    capabilities: vec![CapabilityDeclaration {
                        id: "tool:mytool".parse()?,
                        replaces: None,
                    }],
                }
                .validate()?;
                let envelope = Envelope::new(
                    request.request_id,
                    ProtocolMessage::TerminalResponse {
                        result: TerminalResult::Discovery { manifest },
                    },
                );
                stdout.write_all(&encode_line(&envelope)?)?;
            }
            _ => {}
        }
    }
    Ok(())
}
```

## 2. Testing with the Protocol

Plugin authors can test protocol messages deterministically in standard Rust unit tests:

```rust
#[test]
fn test_manifest_validation() {
    let manifest = CapabilityManifest {
        plugin_id: "rho-plugin-sample".parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        capabilities: vec![CapabilityDeclaration {
            id: "tool:sample".parse().unwrap(),
            replaces: None,
        }],
    };
    assert!(manifest.validate().is_ok());
}
```

## 3. Distribution & Publishing to crates.io

1. Set up a cargo binary crate depending on `rho-sdk`:
   - Package name: `rho-plugin-<name>` or `rho-<name>`
   - `[[bin]] name = "rho-plugin-<name>"`
2. Publish with `cargo publish`.
3. Users install with `cargo install rho-plugin-<name>` (or `rho plugin install rho-plugin-<name>`).
