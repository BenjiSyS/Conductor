# Create a Conductor theme

A theme is a folder containing `theme.json`. Themes contain validated design tokens, never executable code, arbitrary CSS, remote fonts or URLs.

## Start

Copy `examples/theme/theme.json` into your own folder. Give the theme a unique name and semantic version. Define `light`, `dark`, or both. Each is a map of allowed token names to values. Tokens omitted by the theme retain application defaults.

```json
{
  "name": "Forest",
  "version": "1.0.0",
  "author": "Your name",
  "description": "Quiet green accents",
  "light": { "accent": "#285846", "focus": "#285846" },
  "dark": { "accent": "#a8cbb8", "focus": "#a8cbb8" }
}
```

## Tokens and validation

`crates/conductor-tools/src/themes.rs` is the authoritative whitelist. Color tokens include `bg`, `surface`, `border`, `text`, `accent`, `focus`, `success`, `warning`, `danger` and code/agent colors. Typography uses `font-ui`, `font-mono` and `font-size`. Spacing and shapes use `space-unit`, `radius-sm`, `radius`, `radius-lg` and `border-width`. Motion uses `ease` and `duration`.

Colors accept validated color syntax. Lengths use permitted units and bounded values. Durations use milliseconds and are capped. Unknown tokens, scripts, URLs, imports, CSS injection and invalid values are rejected.

## Preview

Install the local folder using the desktop Appearance settings. Select it and inspect light/dark mode, high-DPI scaling, small window layouts, keyboard focus, error/disabled states and reduced motion. Use `npm run dev` for frontend development. A screenshot alone does not establish accessible contrast or keyboard behavior.

## Package, share and update

Publish the theme folder in a Git repository with its license and source information. Tag a version. Pin installations to a verified revision/version. Keep `theme.json` at the package root. Updates must validate the new manifest before activation and retain the old working version for rollback. Uninstall only the managed theme package.

The Rust theme validator has deterministic tests. Native installation, Git-based fetching, preview and rollback still need release-level evidence; see the implementation audit.
