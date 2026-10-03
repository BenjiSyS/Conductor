# UI review evidence

The desktop uses Svelte 5, shared typography/spacing/color tokens and Tauri's native window. The default workspace puts project navigation on the left, the conversation in the center and a compact composer below it. Advanced context, Git, history, memory and settings controls open on demand.

## Observed

- Native Windows first-run wizard rendered at 1240×820. Skip setup opened the workspace without an account. Labelled controls appeared in Windows UI Automation after input.
- Browser preview screenshots show restrained light styling, clear labels, grouped mode/model/effort controls and ordinary focusable controls. Browser fixtures are visibly identified.
- Browser walkthrough tested all eight wizard steps and all settings sections.
- The 760×640 browser test kept the prompt visible and detected no horizontal document overflow. This is browser evidence; native high-DPI/minimum-size behavior remains unverified.
- Keyboard command-palette operation passed a browser test.
- Three initial regressions were reproduced: toast covering Stop, saved Combo absent, overlapping drawers blocking panel switching. The complete browser suite now passes 14/14 against updated source.

Screenshots are generated locally under `target/screenshots` by Playwright. Browser screenshots are preview fixtures; `native-*` screenshots belong to the other builder's desktop smoke against a local scripted provider. Neither proves a live cloud account. Native Goal verification requires the script's file/checkpoint/Test Gate assertions, not a screenshot labelled Complete.

## Remaining

Verify native light/dark/system modes, scaled text, reduced motion, focus traps/restoration, full keyboard operation, contrast, screen-reader semantics, long provider/model names, large histories, validation errors, interrupted streaming, approval workflows and remote layouts. Test the Windows emergency shortcut failure and an alternate binding. A passing frontend compiler alone does not prove accessibility or usability.
