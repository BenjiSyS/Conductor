# MCP

Conductor's shared MCP layer lives in `crates/conductor-tools/src/mcp`. It contains configuration, source catalog, stdio transport, installation and doctor functions. Compatible agents should reuse one configured server through adapters.

An MCP installation must identify its source, dependencies, version, permissions and removal method. Under Full Access, routine authorized setup steps should proceed without repeated prompts; that does not authorize an untrusted source or disable integrity checks.

MCP Doctor must distinguish missing dependency, configuration error, authentication, connection, running state, permissions and outdated/broken integrations. Fix, Reconnect, Update, Disable and Remove need action-specific results. A process spawning is not proof that MCP initialization or tool calls work.

Verification requires a real initialization handshake, tools/list, at least one tool call, cancellation, broken server, missing dependency, malformed response and secure teardown.
