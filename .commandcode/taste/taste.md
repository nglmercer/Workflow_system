# Taste (Continuously Learned by [CommandCode][cmd])

[cmd]: https://commandcode.ai/

# Architecture
- Prefer embedding companion services (LSP, parsers) as library crates with a thin binary entrypoint over spawning separate processes. Avoid process orchestration, path lookups, and JSON-RPC overhead when the same workspace produces both the editor and the service. Confidence: 0.75
