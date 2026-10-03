# Privacy

Conductor requires no Conductor account. Settings, project metadata, conversations, decisions, caches and checkpoints live locally by default. Provider accounts remain provider-specific.

When you send a request, Conductor transmits its selected prompt, instructions and project context to the chosen provider. Provider privacy policies and API terms apply. Review the Context Inspector before sharing sensitive projects. Secret scanning is a defense, not a guarantee that arbitrary confidential information is removed.

Keys use OS credential storage. They are not included in configuration exports. Context, history and cached text are redacted before persistence/transmission. A user's repository remains their repository; Conductor does not move it to a Conductor cloud server.

Remote hosting is an explicit local-machine capability. It requires the host to be on and reachable, authenticated encrypted sessions, project scope and revocable devices. Remote approval, disconnect and encryption tests remain release gates.

Telemetry must be optional, with privacy-respecting defaults. No prompt or source-code collection is authorized by installing Conductor. The application must work without telemetry.

Diagnostic exports require inspection and redaction. Do not send private source, tokens, prompt contents or machine credentials in public bug reports. Deleting an integration must not remove unrelated user software or project files.
