import { expect, test } from '@playwright/test';
import { completeWizard, openProject, shots } from './helpers';

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

  test('stop interrupts a streaming reply', async ({ page }) => {
    await page.getByLabel('Prompt').fill('Explain everything');
    await page.getByLabel('Prompt').press('Enter');
    await page.getByRole('button', { name: 'Stop' }).click();
    await expect(page.getByRole('article', { name: 'assistant message' }).getByText('stopped')).toBeVisible({
      timeout: 10_000,
    });
  });

  test('model picker lists combos and models; effort only shows declared levels', async ({ page }) => {
    await page.getByRole('button', { name: /GPT-5/ }).first().click();
    const list = page.getByRole('listbox', { name: 'Choose a model or Combo' });
    await expect(list.getByText('Combos', { exact: true })).toBeVisible();
    await expect(list.getByText('Balanced')).toBeVisible();
    await page.screenshot({ path: `${shots}/07-model-picker.png` });
    await list.getByLabel('Search models').fill('mini');
    await list.getByRole('option', { name: /GPT-5 mini/ }).click();
    const effort = page.getByLabel('Effort', { exact: true });
    await expect(effort).toBeVisible();
    await expect(effort.locator('option')).toHaveText(['Auto', 'Minimal', 'Low', 'Medium', 'High']);
    // A Combo uses per-member effort.
    await page
      .getByRole('button', { name: /GPT-5 mini/ })
      .first()
      .click();
    await page.getByRole('option', { name: /Balanced/ }).click();
    await expect(page.getByLabel('Effort', { exact: true })).toBeHidden();
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
    await page.getByRole('button', { name: 'Goal', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Start a Goal' })).toBeVisible();
    await page.getByRole('button', { name: /Checks/ }).click();
    await page.getByLabel(/Definition of Done/).fill('cargo test');
    await page.getByLabel('Prompt').fill('Build a small multiplayer lobby');
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
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const s = page.getByRole('dialog', { name: 'Settings' });
    const sections: [string, string][] = [
      ['General', 'General'],
      ['Appearance', 'Appearance'],
      ['Updates', 'Updates'],
      ['Providers', 'Providers'],
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
