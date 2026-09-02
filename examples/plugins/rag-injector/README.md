# RAG Context Injector Plugin for `rho`

An example RAG plugin in Python demonstrating how plugins can dynamically query a vector index or documentation database and inject `extra_context` documents into `rho`'s completion requests.

## Configuration in `config.toml`

```toml
[plugins.rag_injector]
enabled = true
command = "python3"
args = ["/path/to/rho/examples/plugins/rag-injector/rag.py"]
```
