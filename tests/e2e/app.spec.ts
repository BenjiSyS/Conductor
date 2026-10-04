import { expect, test } from '@playwright/test';
import { completeWizard, openProject, openSettings, shots } from './helpers';

test.describe('first run', () => {
  test('wizard walks every step and can be skipped', async ({ page }) => {
    await page.goto('/');
    const wizard = page.getByRole('dialog', { name: 'Set up Conductor' });
    await expect(wizard.getByRole('heading', { name: 'Welcome to Conductor' })).toBeVisible();
    await page.screenshot({ path: `${shots}/01-wizard-welcome.png` });
    await page.getByRole('button', { name: 'Get started' }).click();
    await expect(wizard.getByText('How hard should Conductor push this machine?')).toBeVisible();
    await expect(wizard.getByText('recommended')).toBeVisible();
    await page.getByRole('button', { name: 'Continue' }).click();
    await expect(wizard.getByRole('heading', { name: 'Connect a provider' })).toBeVisible();
    await page.screenshot({ path: `${shots}/02-wizard-provider.png` });
    // Validation: OpenAI needs a key.
    await expect(page.getByRole('button', { name: 'Test & connect' })).toBeDisabled();
    await page.getByRole('radio', { name: 'Ollama (local)' }).click();
    await expect(page.getByLabel('Base URL')).toHaveValue('http://localhost:11434/v1');
    await page.getByRole('button', { name: 'Test & connect' }).click();
    await expect(page.getByText('Ollama connected')).toBeVisible();
    for (const heading of [
      'One model or a Combo?',
      'How much should agents ask first?',
      'Remote access',
      'Save tokens',
    ]) {
      await page.getByRole('button', { name: 'Continue' }).click();
      await expect(wizard.getByRole('heading', { name: heading })).toBeVisible();
    }
    await page.screenshot({ path: `${shots}/03-wizard-optimization.png` });
    await page.getByRole('button', { name: 'Continue' }).click();
    await expect(wizard.getByRole('heading', { name: "You're ready" })).toBeVisible();
    await page.getByRole('button', { name: 'Done' }).click();
    await expect(wizard).toBeHidden();
  });

  test('skip setup goes straight to the app', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Skip setup' }).click();
    await expect(page.getByRole('heading', { name: 'Conductor', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open folder' })).toBeVisible();
    await page.screenshot({ path: `${shots}/04-welcome-empty.png` });
  });
});

test.describe('setup in the background', () => {
  test('wizard checks the computer while open, installs Git, and offers local models', async ({ page }) => {
    await page.goto('/');
    const ready = page.getByRole('region', { name: 'Getting your computer ready' });
    await expect(ready.getByText('Git — not found')).toBeVisible();
    await expect(ready.getByText('Local models (work offline): Ollama · 3 models')).toBeVisible();
    await expect(ready.getByText('Signed-in AI apps: Gemini (Antigravity CLI)')).toBeVisible();
    await ready.getByRole('button', { name: 'Install' }).click();
    await expect(ready.getByText('Git 2.49.0')).toBeVisible();
    await page.screenshot({ path: `${shots}/26-wizard-ready.png` });
    // The provider step offers the local model and signed-in app with one click.
    await page.getByRole('button', { name: 'Get started' }).click();
    await page.getByRole('button', { name: 'Continue' }).click();
    await page.getByRole('button', { name: 'Connect Ollama' }).click();
    await expect(page.getByText('Ollama connected · 1 model (works offline)')).toBeVisible();
  });
});

test.describe('daily use', () => {
  test.beforeEach(async ({ page }) => {
    await completeWizard(page);
    await openProject(page);
  });

  test('chat streams a reply and shows context budget', async ({ page }) => {
    await expect(page.getByText('Ask anything about demo-app')).toBeVisible();
    await page.screenshot({ path: `${shots}/05-project-home.png` });
    const prompt = page.getByLabel('Prompt');
    await prompt.fill('Where is the entry point?');
    await expect(page.getByRole('button', { name: /Context budget/ })).toBeVisible();
    await prompt.press('Enter');
    await expect(page.getByRole('article', { name: 'user message' })).toContainText('Where is the entry point?');
    await expect(page.getByRole('article', { name: 'assistant message' })).toContainText('browser preview', {
      timeout: 15_000,
    });
    await expect(page.getByRole('button', { name: 'Send' })).toBeVisible({ timeout: 15_000 });
    await expect(page.getByRole('navigation', { name: 'Projects and conversations' })).toContainText(
      'Where is the entry point?',
    );
    await page.screenshot({ path: `${shots}/06-chat.png` });
  });

  test('a 5-million-character paste becomes a chip and sends without freezing', async ({ page }) => {
    const prompt = page.getByLabel('Prompt');
    await prompt.fill('Summarize this: ');
    const started = Date.now();
    await prompt.evaluate((el) => {
      const dt = new DataTransfer();
      dt.setData('text/plain', 'log line\n'.repeat(555_556).slice(0, 5_000_000));
      el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
    });
    await expect(prompt).toHaveValue(
      /^Summarize this: \[Pasted text #1 · 5,000,000 chars · 555,556 lines · paste:[0-9a-f]{32}\]$/,
    );
    expect(Date.now() - started).toBeLessThan(5_000);
    await prompt.press('Enter');
    const sent = page.getByRole('article', { name: 'user message' });
    await expect(sent).toContainText('[Pasted text #1 · 5,000,000 chars · 555,556 lines]');
    await expect(sent).not.toContainText('paste:');
    await expect(page.getByRole('button', { name: 'Send' })).toBeVisible({ timeout: 15_000 });
    // Small pastes stay as normal text.
    await prompt.evaluate((el) => {
      const dt = new DataTransfer();
      dt.setData('text/plain', 'short');
      el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
    });
    await expect(prompt).not.toHaveValue(/Pasted text/);
  });

  test('only Chat and Agent are modes; /goal and /plan start the others', async ({ page }) => {
    const modes = page.getByRole('group', { name: 'Mode' });
    await expect(modes.getByRole('button')).toHaveText(['Chat', 'Agent']);
    const prompt = page.getByLabel('Prompt');
    // Typing "/" offers the commands; Tab completes the first match.
    await prompt.fill('/');
    const menu = page.getByRole('listbox', { name: 'Commands' });
    await expect(menu.getByRole('option')).toHaveCount(2);
    await prompt.fill('/pl');
    await expect(menu.getByRole('option')).toHaveCount(1);
    await prompt.press('Tab');
    await expect(prompt).toHaveValue('/plan ');
    await expect(page.getByRole('heading', { name: 'Plan', exact: true })).toBeVisible();
    await page.screenshot({ path: `${shots}/31-slash-plan.png` });
    // An empty command explains what to add instead of sending.
    await prompt.press('Enter');
    await expect(page.getByText('Describe what to plan after /plan')).toBeVisible();
    await prompt.fill('/plan Split the parser into modules');
    await prompt.press('Enter');
    const sent = page.getByRole('article', { name: 'user message' });
    await expect(sent).toContainText('Split the parser into modules');
    await expect(sent).not.toContainText('/plan');
    await expect(page.getByRole('button', { name: 'Send' })).toBeVisible({ timeout: 15_000 });
    // The mode returns to Chat afterwards.
    await expect(modes.getByRole('button', { name: 'Chat' })).toHaveAttribute('aria-pressed', 'true');
    // The command palette prefills /goal.
    await page.keyboard.press('Control+k');
    await page.keyboard.type('Start a Goal');
    await page.keyboard.press('Enter');
    await expect(prompt).toHaveValue('/goal ');
    await expect(page.getByRole('button', { name: 'Start Goal' })).toBeVisible();
  });

  test('stop interrupts a streaming reply', async ({ page }) => {
    await page.getByLabel('Prompt').fill('Explain everything');
    await page.getByLabel('Prompt').press('Enter');
    await page.getByRole('button', { name: 'Stop' }).click();
    await expect(page.getByRole('article', { name: 'assistant message' }).getByText('stopped')).toBeVisible({
      timeout: 10_000,
    });
  });

  test('model picker lists combos and models; effort only shows declared levels', async ({ page }) => {
    await page.getByRole('button', { name: /GPT-6/ }).first().click();
    const list = page.getByRole('listbox', { name: 'Choose a model or Combo' });
    await expect(list.getByText('Combos', { exact: true })).toBeVisible();
    await expect(list.getByText('Balanced')).toBeVisible();
    await page.screenshot({ path: `${shots}/07-model-picker.png` });
    await list.getByLabel('Search models').fill('luna');
    await list.getByRole('option', { name: /GPT-6 Luna/ }).click();
    // Effort slider: Auto plus only the levels the model declares.
    const effort = page.getByRole('slider', { name: 'Effort', exact: true });
    await expect(effort).toHaveAttribute('max', '4');
    await expect(effort).toHaveAttribute('aria-valuetext', 'Auto');
    await effort.focus();
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
    await expect(effort).toHaveAttribute('aria-valuetext', 'Low');
    await page.keyboard.press('End');
    await expect(effort).toHaveAttribute('aria-valuetext', 'High');
    // A Combo offers every level its members support (members skip the rest).
    await page
      .getByRole('button', { name: /GPT-6 Luna/ })
      .first()
      .click();
    await page.getByRole('option', { name: /Balanced/ }).click();
    // The chosen level carries over when the Combo supports it.
    await expect(page.getByRole('slider', { name: 'Effort', exact: true })).toHaveAttribute('aria-valuetext', 'High');
  });

  test('context inspector explains what is sent', async ({ page }) => {
    await page.getByLabel('Prompt').fill('route requests');
    await page.getByRole('button', { name: /Context budget/ }).click();
    const inspector = page.getByRole('complementary', { name: 'Context Inspector' });
    await expect(inspector.getByText('Included')).toBeVisible();
    await expect(inspector.getByText('src/main.rs')).toBeVisible();
    await expect(inspector.getByText('avoided vs. naive')).toBeVisible();
    await page.screenshot({ path: `${shots}/08-inspector.png` });
  });

  test('goal: questions, decide for me, live progress, verified completion', async ({ page }) => {
    await page.getByLabel('Prompt').fill('/goal Build a small multiplayer lobby');
    await expect(page.getByRole('heading', { name: 'Start a Goal' })).toBeVisible();
    await page.getByRole('button', { name: /Checks/ }).click();
    await page.getByLabel(/Definition of Done/).fill('cargo test');
    await page.getByRole('button', { name: 'Start Goal' }).click();
    const card = page.getByRole('region', { name: 'A few questions before starting' });
    await expect(card.getByText('Which platforms matter?')).toBeVisible();
    await page.screenshot({ path: `${shots}/09-goal-questions.png` });
    await card.getByRole('button', { name: 'Web' }).click();
    await card.getByRole('button', { name: 'Decide for me' }).first().click(); // decide all
    await expect(page.getByText('Goal Contract')).toBeVisible();
    await expect(page.getByText('Implement the change')).toBeVisible({ timeout: 10_000 });
    await page.screenshot({ path: `${shots}/10-goal-running.png` });
    await expect(page.locator('.badge.state')).toHaveText(/Complete/, { timeout: 20_000 });
    await page.getByRole('button', { name: /Goal Contract/ }).click();
    await expect(page.getByText('cargo test').first()).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Verification' })).toBeVisible();
    await page.screenshot({ path: `${shots}/11-goal-complete.png`, fullPage: true });
  });

  test('command palette runs actions by keyboard', async ({ page }) => {
    await page.keyboard.press('Control+k');
    const palette = page.getByRole('dialog', { name: 'Command palette' });
    await expect(palette).toBeVisible();
    await page.keyboard.type('perm');
    await page.screenshot({ path: `${shots}/12-palette.png` });
    await page.keyboard.press('Enter');
    await expect(
      page.getByRole('dialog', { name: 'Settings' }).getByRole('heading', { name: 'Permissions' }),
    ).toBeVisible();
  });

  test('settings: every section opens', async ({ page }) => {
    await openSettings(page);
    const s = page.getByRole('dialog', { name: 'Settings' });
    const sections: [string, string][] = [
      ['General', 'General'],
      ['Appearance', 'Appearance'],
      ['Updates', 'Updates'],
      ['Providers', 'Providers'],
      ['Usage', 'Usage'],
      ['Combos', 'Combos'],
      ['Instructions', 'Instructions'],
      ['Permissions', 'Permissions'],
      ['Privacy & data', 'Privacy & data'],
      ['Optimization', 'Optimization'],
      ['MCP, skills & plugins', 'MCP, skills & plugins'],
      ['Environment', 'Environment'],
      ['Remote access', 'Remote access'],
      ['About', 'Conductor'],
    ];
    for (const [tab, heading] of sections) {
      await s.getByRole('navigation').getByRole('button', { name: tab, exact: true }).click();
      await expect(s.getByRole('heading', { name: heading, exact: true })).toBeVisible();
    }
    await s.getByRole('navigation').getByRole('button', { name: 'Combos', exact: true }).click();
    await page.screenshot({ path: `${shots}/13-settings-combos.png` });
  });

  test('usage: themed per provider, API limits, opt-in subscription sign-in', async ({ page }) => {
    await openSettings(page);
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'Usage', exact: true }).click();
    const panel = s.getByTestId('usage-panel');
    const tabs = s.getByRole('tablist', { name: 'AI provider' });

    // Each provider has its own theme.
    await expect(panel).toHaveAttribute('data-brand', 'claude');
    const bg = async () => panel.evaluate((el) => getComputedStyle(el).backgroundColor);
    const claudeBg = await bg();
    await tabs.getByRole('tab', { name: 'ChatGPT' }).click();
    await expect(panel).toHaveAttribute('data-brand', 'chatgpt');
    // The panel animates between themes; wait for the new colour.
    await expect.poll(bg).not.toBe(claudeBg);

    // API rate limits come from the provider's last response.
    await expect(panel.getByText('487 of 500 left')).toBeVisible();
    await expect(panel.getByText(/This session: 3 replies/)).toBeVisible();

    // Subscription sign-in is off until the user accepts the warning.
    await expect(panel.getByRole('button', { name: 'Sign in with ChatGPT' })).toHaveCount(0);
    page.once('dialog', (d) => d.dismiss());
    await s.getByRole('switch', { name: 'Subscription sign-in' }).click();
    await expect(s.getByRole('switch', { name: 'Subscription sign-in' })).toHaveAttribute('aria-checked', 'false');
    page.once('dialog', (d) => {
      expect(d.message()).toContain('not supported');
      void d.accept();
    });
    await s.getByRole('switch', { name: 'Subscription sign-in' }).click();
    await expect(s.getByRole('switch', { name: 'Subscription sign-in' })).toHaveAttribute('aria-checked', 'true');

    for (const [id, label, meter, used] of [
      ['claude', 'Claude', '5-hour session used', '42'],
      ['chatgpt', 'ChatGPT', 'Weekly window used', '31'],
      ['gemini', 'Gemini', 'gemini-2.5-pro used', '25'],
    ] as const) {
      await tabs.getByRole('tab', { name: label }).click();
      await expect(panel).toHaveAttribute('data-brand', id);
      await panel.getByRole('button', { name: `Sign in with ${label}` }).click();
      await expect(panel.getByRole('meter', { name: meter })).toHaveAttribute('aria-valuenow', used);
      await expect(panel.getByText('you@example.com')).toBeVisible();
      await page.screenshot({ path: `${shots}/22-usage-${id}.png` });
    }
    // Near-limit windows are highlighted.
    await tabs.getByRole('tab', { name: 'Claude' }).click();
    await expect(panel.locator('.meter span.hot')).toHaveCount(1);

    await panel.getByRole('button', { name: 'Sign out' }).click();
    await expect(panel.getByRole('button', { name: 'Sign in with Claude' })).toBeVisible();
  });

  test('connect a signed-in app (no API key) and see it under its brand', async ({ page }) => {
    await openSettings(page);
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'Providers', exact: true }).click();
    await s.getByRole('button', { name: 'Add provider' }).click();
    const apps = s.getByRole('region', { name: 'Apps you already have' });
    await expect(apps.getByText('Gemini (Antigravity CLI)')).toBeVisible();
    // Not installed apps are not offered.
    await expect(apps.getByText('Claude (Claude Code)')).toHaveCount(0);
    await apps.getByRole('button', { name: 'Connect Gemini (Antigravity CLI)' }).click();
    await expect(page.getByText('Gemini (Antigravity CLI) connected · 3 models')).toBeVisible();
    await page.screenshot({ path: `${shots}/23-bridge-connect.png` });

    await s.getByRole('navigation').getByRole('button', { name: 'Usage', exact: true }).click();
    await s.getByRole('tablist', { name: 'AI provider' }).getByRole('tab', { name: 'Gemini' }).click();
    const block = s.getByRole('region', { name: 'Gemini (Antigravity CLI) API usage' });
    await expect(block.getByText('Signed-in app')).toBeVisible();
    // Bridged apps are not listed under "Other".
    await s.getByRole('tablist', { name: 'AI provider' }).getByRole('tab', { name: 'Other' }).click();
    await expect(s.getByText('Gemini (Antigravity CLI)')).toHaveCount(0);
  });

  test('profile menu opens usage, providers and settings; xAI (Grok) gets its own theme', async ({ page }) => {
    await page.getByRole('button', { name: 'Profile and settings' }).click();
    const menu = page.getByRole('menu', { name: 'Profile' });
    await expect(menu.getByText('Alex')).toBeVisible();
    await expect(menu.getByText('Local profile · no Conductor account')).toBeVisible();
    await expect(menu.getByText('OpenAI')).toBeVisible(); // usage at a glance
    await page.screenshot({ path: `${shots}/24-profile-menu.png` });
    await menu.getByRole('menuitem', { name: 'Providers & accounts' }).click();
    const s = page.getByRole('dialog', { name: 'Settings' });
    await expect(s.getByRole('heading', { name: 'Providers', exact: true })).toBeVisible();
    await expect(menu).toBeHidden();
    // Add xAI with an API key; the base URL is fixed, so it isn't asked for.
    await s.getByRole('button', { name: 'Add provider' }).click();
    await s.getByRole('radio', { name: 'xAI (Grok)' }).click();
    await expect(s.getByLabel('Base URL')).toHaveCount(0);
    await s.getByLabel('API key').fill('xai-test-key');
    await s.getByRole('button', { name: 'Test & connect' }).click();
    await expect(page.getByText('xAI (Grok) connected · 2 models')).toBeVisible();
    await s.getByRole('navigation').getByRole('button', { name: 'Usage', exact: true }).click();
    await s.getByRole('tablist', { name: 'AI provider' }).getByRole('tab', { name: 'Grok' }).click();
    await expect(s.getByTestId('usage-panel')).toHaveAttribute('data-brand', 'grok');
    await expect(s.getByRole('region', { name: 'xAI (Grok) API usage' })).toBeVisible();
    await page.screenshot({ path: `${shots}/25-usage-grok.png` });
    // Escape closes the menu without side effects.
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Profile and settings' }).click();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('menu', { name: 'Profile' })).toBeHidden();
  });

  test('dark and light mode switch from the profile menu and cover every screen', async ({ page }) => {
    const bg = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    const dark = async () => {
      const [r, g, b] = (await bg()).match(/[0-9]+/g)!.map(Number);
      return r + g + b < 200;
    };
    await page.getByRole('button', { name: 'Profile and settings' }).click();
    const theme = page.getByRole('group', { name: 'Theme' });
    await theme.getByRole('button', { name: /Dark/ }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await expect.poll(dark).toBe(true);
    await page.screenshot({ path: `${shots}/28-dark-profile.png` });
    await page.keyboard.press('Escape');
    await page.getByLabel('Prompt').fill('Where is the entry point?');
    await page.getByLabel('Prompt').press('Enter');
    await expect(page.getByRole('button', { name: 'Send' })).toBeVisible({ timeout: 15_000 });
    await page.screenshot({ path: `${shots}/29-dark-chat.png` });
    await openSettings(page);
    const s = page.getByRole('dialog', { name: 'Settings' });
    for (const tab of ['Usage', 'Providers', 'Combos', 'Appearance']) {
      await s.getByRole('navigation').getByRole('button', { name: tab, exact: true }).click();
      await page.screenshot({ path: `${shots}/30-dark-${tab.toLowerCase()}.png` });
    }
    // The chosen mode persists in preferences and Light switches back.
    await s.getByRole('navigation').getByRole('button', { name: 'Appearance', exact: true }).click();
    await expect(s.getByRole('button', { name: 'Dark', exact: true })).toHaveAttribute('aria-pressed', 'true');
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Profile and settings' }).click();
    await page.getByRole('group', { name: 'Theme' }).getByRole('button', { name: /Light/ }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await expect.poll(dark).toBe(false);
  });

  test('full access is obvious and revocable', async ({ page }) => {
    await page.getByRole('button', { name: /^Permissions:/ }).click();
    await page.getByRole('radio', { name: /Full Access/ }).click();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('button', { name: 'Permissions: Full Access' })).toBeVisible();
    await page.getByRole('button', { name: 'Permissions: Full Access' }).click();
    await page.getByRole('radio', { name: /^Ask/ }).click();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('button', { name: 'Permissions: Ask' })).toBeVisible();
  });

  test('combo editor creates and saves a Combo', async ({ page }) => {
    await page.keyboard.press('Control+,');
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'Combos', exact: true }).click();
    await s.getByRole('button', { name: 'New Combo' }).click();
    const editor = page.getByRole('dialog', { name: 'Edit Combo' });
    await editor.getByLabel('Name').fill('Review heavy');
    await editor.getByRole('button', { name: 'Reviewer' }).first().click();
    await editor.getByRole('button', { name: 'Add reserve' }).click();
    await page.screenshot({ path: `${shots}/14-combo-editor.png` });
    await editor.getByRole('button', { name: 'Save Combo' }).click();
    await expect(s.getByText('Review heavy')).toBeVisible();
  });

  test('MCP: a remote connector connects by URL with one click', async ({ page }) => {
    await openSettings(page);
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'MCP, skills & plugins', exact: true }).click();
    await s.getByLabel('Connector URL').fill('https://mcp.linear.app/mcp');
    await s.getByRole('button', { name: 'Connect', exact: true }).click();
    await expect(page.getByText('linear connected', { exact: true })).toBeVisible();
    await expect(s.getByText('linear connected · 21 tool(s)')).toBeVisible();
    await expect(s.getByRole('button', { name: 'Sign in' })).toBeVisible();
    await page.screenshot({ path: `${shots}/27-remote-connector.png` });
  });

  test('MCP: catalog search and install with doctor result', async ({ page }) => {
    await page.keyboard.press('Control+,');
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'MCP, skills & plugins' }).click();
    await s.getByLabel('Search MCP catalog').fill('blender');
    await s.getByLabel('Search MCP catalog').fill('');
    const row = s.locator('.cat').filter({ hasText: 'Everything (test server)' });
    await row.getByRole('button', { name: 'Add' }).click();
    await expect(page.getByText(/installed and connected/)).toBeVisible();
    await expect(s.getByText('everything', { exact: true })).toBeVisible();
    await page.screenshot({ path: `${shots}/15-mcp.png` });
  });

  test('light theme and small window stay usable', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.screenshot({ path: `${shots}/16-light.png` });
    await page.setViewportSize({ width: 760, height: 640 });
    await expect(page.getByLabel('Prompt')).toBeVisible();
    await page.screenshot({ path: `${shots}/17-small.png` });
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    expect(overflow).toBe(false);
  });

  test('git, history and memory panels open', async ({ page }) => {
    await page.getByLabel('Prompt').fill('hello');
    await page.getByLabel('Prompt').press('Enter');
    await expect(page.getByRole('button', { name: 'Send' })).toBeVisible({ timeout: 15_000 });
    await page.getByRole('button', { name: 'Git panel' }).click();
    await expect(page.getByRole('complementary', { name: 'Git' }).getByText('src/main.rs')).toBeVisible();
    await page.getByRole('button', { name: 'History panel' }).click();
    await expect(page.getByRole('complementary', { name: 'History' })).toBeVisible();
    await page.getByRole('button', { name: 'Project memory panel' }).click();
    await page.getByLabel('Decision topic').fill('engine');
    await page.getByLabel('Decision value').fill('Godot');
    await page.getByRole('button', { name: 'Add decision' }).click();
    await expect(page.getByRole('complementary', { name: 'Project memory' }).getByText('Godot')).toBeVisible();
    await page.screenshot({ path: `${shots}/18-panels.png` });
  });
});

test.describe('previews and GitHub', () => {
  test.beforeEach(async ({ page }) => {
    await completeWizard(page);
    await openProject(page);
    await page.getByLabel('Prompt').fill('hello');
    await page.getByLabel('Prompt').press('Enter');
    await expect(page.getByRole('button', { name: 'Send' })).toBeVisible({ timeout: 15_000 });
  });

  test('GitHub tab lists pull requests, issues and CI runs', async ({ page }) => {
    await page.getByRole('button', { name: 'Git panel' }).click();
    const git = page.getByRole('complementary', { name: 'Git' });
    await git.getByRole('button', { name: 'GitHub' }).click();
    await expect(git.getByRole('link', { name: '#12 Add lobby screen' })).toBeVisible();
    await expect(git.getByRole('link', { name: '#7 Crash on resize' })).toBeVisible();
    await expect(git.getByText(/success/)).toBeVisible();
  });

  test('preview: point at a region and turn it into prompt feedback', async ({ page }) => {
    await page.getByRole('button', { name: 'Preview panel' }).click();
    const panel = page.getByRole('complementary', { name: 'Preview' });
    await expect(panel.locator('iframe')).toHaveAttribute('src', 'http://localhost:1420');
    await panel.getByRole('button', { name: 'Comment' }).click();
    const overlay = panel.locator('.overlay');
    const box = (await overlay.boundingBox())!;
    await page.mouse.move(box.x + 40, box.y + 40);
    await page.mouse.down();
    await page.mouse.move(box.x + 200, box.y + 120);
    await page.mouse.up();
    await panel.getByLabel('Preview comment').fill('Make this header bolder');
    await page.screenshot({ path: `${shots}/19-preview-feedback.png` });
    await panel.getByRole('button', { name: 'Add to prompt' }).click();
    await expect(page.getByLabel('Prompt')).toHaveValue(
      /\[Preview feedback on http:\/\/localhost:1420 — region x=40, y=40, 160×80px.*\]: Make this header bolder/,
    );
    await expect(page.getByRole('button', { name: 'Agent', exact: true })).toHaveAttribute('aria-pressed', 'true');
    // Only local dev servers can be previewed.
    await panel.getByLabel('Preview URL').fill('https://example.com');
    await panel.getByRole('button', { name: 'Go' }).click();
    await expect(page.getByText('Previews show local dev servers only')).toBeVisible();
  });
});
