# Providers and authentication

Core types are provider-neutral. `ProviderConfig` contains metadata only. `ProviderRequest` contains model, messages, instructions and supported effort. `StreamEvent` delivers text, usage and verified completion. Cancellation and structured errors remain separate from ordinary response text.

| Adapter | API family | Authentication |
| --- | --- | --- |
| OpenAI | Responses API; model catalog | Bearer API key |
| Anthropic | Messages API with SSE; model catalog | x-api-key and version header |
| Gemini | streamGenerateContent with SSE; model catalog | x-goog-api-key |
| Custom / Local | OpenAI-compatible chat/completions and models | Optional bearer key |

Built-in providers use their official HTTPS hosts. Custom endpoints require HTTPS, except localhost HTTP for local software. Redirects are disabled to prevent credential forwarding. URL userinfo, query credentials and fragments are rejected.

Catalog data often omits effort and capability metadata. Unknown capabilities remain unknown. Trusted catalog enrichment must be model-specific, versioned and evidence-based. Unsupported levels are rejected. Highest-effort policy must respect explicit user choice.

The core catalog adapter follows Anthropic `after_id` and Gemini `pageToken` pagination, encodes each cursor as one query value, and deduplicates model identities. It rejects repeated/malformed cursors and catalogs exceeding 64 pages, 1,000 unique models or 2 MB total downloaded bytes. Limits fail explicitly rather than silently returning an incomplete catalog. Requests and body reads remain cancellable; every page retains provider-specific authentication.

Anthropic catalog `display_name`, `max_input_tokens`, image capability and individually declared effort levels populate model metadata. Effort requests use `output_config.effort`; highest levels still require explicit allowance. Gemini catalogs retain only models declaring `generateContent`, and preserve their display name and input limit. Protocol fixtures verify these mappings; they are not live account tests. References: [Anthropic model listing](https://platform.claude.com/docs/en/api/models/list), [Anthropic effort](https://platform.claude.com/docs/en/build-with-claude/effort), [Gemini model listing](https://ai.google.dev/api/models).

Protocol references: [OpenAI Responses](https://platform.openai.com/docs/api-reference/responses), [Anthropic streaming](https://platform.claude.com/docs/en/build-with-claude/streaming), [Gemini generation](https://ai.google.dev/api/generate-content).

## Authentication gates

Fixture tests verify requests, headers, decoding, cancellation, rate limits and rejection behavior. They do **not** prove a live account or OS credential manager works. Before claiming production readiness, record each provider's valid/invalid/revoked key, network failure, rate limit, restart, reconnect and interrupted-task resume tests.

OAuth may be offered only where officially allowed and technically supported. Required evidence includes fresh login, refresh, expiration, revocation, restart, background execution, multiple projects, resume, logout and reconnect. No OAuth path is verified by the core protocol tests.

Never extract credentials from another app or claim subscription access from API-key support. External accounts and provider developer registrations may be needed. Enter test credentials through native setup; never put them in repository files, docs, chat or logs.
