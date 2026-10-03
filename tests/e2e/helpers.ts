import { expect, type Page } from '@playwright/test';

export const shots = 'target/screenshots';

/** Complete the setup wizard quickly, connecting a provider. */
export async function completeWizard(page: Page) {
  await page.goto('/');
  await expect(page.getByRole('dialog', { name: 'Set up Conductor' })).toBeVisible();
  await page.getByRole('button', { name: 'Get started' }).click();
  await page.getByRole('button', { name: 'Continue' }).click(); // performance
  await page.getByLabel('API key').fill('sk-test-preview-key');
  await page.getByRole('button', { name: 'Test & connect' }).click();
  await expect(page.getByText('OpenAI connected')).toBeVisible();
  await page.getByRole('button', { name: 'Continue' }).click(); // providers
  await page.getByRole('button', { name: 'Continue' }).click(); // models
  await page.getByRole('button', { name: 'Continue' }).click(); // permissions
  await page.getByRole('button', { name: 'Continue' }).click(); // remote
  await page.getByRole('button', { name: 'Continue' }).click(); // optimization
  await page.getByRole('button', { name: 'Done' }).click();
  await expect(page.getByRole('dialog', { name: 'Set up Conductor' })).toBeHidden();
}

export async function openProject(page: Page, path = 'C:\\Projects\\demo-app') {
  page.once('dialog', (d) => d.accept(path));
  await page.getByRole('button', { name: 'Open folder' }).first().click();
  await expect(page.getByRole('heading', { name: 'demo-app', exact: true })).toBeVisible();
}
