// Scripted OpenAI-compatible server for end-to-end tests of the real
// desktop app. It speaks the same SSE wire format as OpenAI-compatible
// providers (Ollama, LM Studio, vLLM…) and answers deterministically based
// on what Conductor sends, so the full Rust pipeline — provider streaming,
// agent tools, Goal planning, review and the Test Gate — runs for real.
//
// Usage: node scripts/mock-llm.mjs [port]
import http from 'node:http';

const port = Number(process.argv[2] ?? 18080);
// MOCK_FAIL=429 makes every chat request fail (rate limit) to exercise
// provider failover; models still list so the provider can connect.
const failWith = Number(process.env.MOCK_FAIL ?? 0);
const log = [];

function reply(body) {
  const messages = body.messages ?? [];
  const system = messages.find((m) => m.role === 'system')?.content ?? '';
  const last = [...messages].reverse().find((m) => m.role === 'user')?.content ?? '';
  const all = messages.map((m) => m.content).join('\n');
  if (last.includes('Ask zero questions if the task is clear')) {
    return JSON.stringify({
      questions: [
        {
          id: 'q1',
          text: 'Which platforms matter?',
          priority: 'blocking',
          options: ['Windows', 'Web'],
          default: 'Windows',
          topic: 'platforms',
        },
        { id: 'q2', text: 'May I run the tests?', priority: 'blocking', options: [], topic: '' },
      ],
    });
  }
  if (last.includes('Reply with JSON only') && last.includes('"tasks"')) {
    return JSON.stringify({
      tasks: [
        { id: 'write', title: 'Create the greeting module', role: 'coder', important: true, files: ['greeting.txt'] },
      ],
    });
  }
  if (last.includes('Reply PASS or FAIL')) return 'PASS\nThe work meets the criterion.';
  if (last.includes('Independently review')) return 'APPROVE — the change is small and correct.';
  if (last.startsWith('Tool results:'))
    return all.includes('long command')
      ? '<done>Finished the long command.</done>'
      : '<done>Created greeting.txt and confirmed Git is available.</done>';
  if (system.includes('You can use tools') && all.includes('long command')) {
    return process.platform === 'win32' ? '<run>ping -n 40 127.0.0.1</run>' : '<run>sleep 40</run>';
  }
  if (system.includes('You can use tools')) {
    return 'I will create the file.\n<write_file path="greeting.txt">Hello from Conductor\n</write_file>\n<run>git --version</run>';
  }
  if (all.includes('UNTRUSTED') || system.includes('Conductor')) {
    return 'The entry point is **`src/main.rs`**. It calls `run()` which wires the router.\n\n```rust\nfn main() {\n    app::run();\n}\n```';
  }
  return 'Hello from the mock provider.';
}

const server = http.createServer((req, res) => {
  let data = '';
  req.on('data', (c) => (data += c));
  req.on('end', async () => {
    const auth = req.headers.authorization ?? '';
    if (req.url?.endsWith('/models')) {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ data: [{ id: 'mock-coder' }, { id: 'mock-reviewer' }] }));
      return;
    }
    if (req.url?.endsWith('/chat/completions') && req.method === 'POST') {
      let body = {};
      try {
        body = JSON.parse(data);
      } catch {
        res.writeHead(400).end();
        return;
      }
      if (failWith) {
        log.push({ model: body.model, failed: failWith });
        res.writeHead(failWith, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ error: { message: 'rate limited (mock)' } }));
        return;
      }
      const text = reply(body);
      const sys = (body.messages ?? []).find((m) => m.role === 'system')?.content ?? '';
      const slow = JSON.stringify(body).includes('SLOW');
      log.push({
        model: body.model,
        effort: body.reasoning_effort ?? null,
        auth: auth ? 'present' : 'none',
        chars: data.length,
        caveman: sys.includes('# Response style'),
        reply: text.slice(0, 60),
      });
      res.writeHead(200, {
        'content-type': 'text/event-stream',
        'cache-control': 'no-cache',
        // Same rate-limit headers OpenAI sends.
        'x-ratelimit-limit-requests': '500',
        'x-ratelimit-remaining-requests': String(500 - log.length),
        'x-ratelimit-reset-requests': '120ms',
        'x-ratelimit-limit-tokens': '30000',
        'x-ratelimit-remaining-tokens': '29500',
        'x-ratelimit-reset-tokens': '1s',
      });
      // Stream in small chunks like a real provider.
      for (let i = 0; i < text.length; i += 24) {
        res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: text.slice(i, i + 24) } }] })}\n\n`);
        await new Promise((r) => setTimeout(r, slow ? 350 : 15));
      }
      res.write(
        `data: ${JSON.stringify({ usage: { prompt_tokens: Math.round(data.length / 4), completion_tokens: Math.round(text.length / 4) } })}\n\n`,
      );
      res.write('data: [DONE]\n\n');
      res.end();
      return;
    }
    if (req.url === '/__log') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(log));
      return;
    }
    res.writeHead(404).end();
  });
});
server.listen(port, '127.0.0.1', () => console.log(`mock LLM on http://127.0.0.1:${port}/v1`));
