import { expect, test, type Page } from '@playwright/test';

const SHOTS = process.env.E2E_SHOTS_DIR;

async function login(page: Page) {
	await page.goto('/');
	await expect(page).toHaveURL(/\/login$/);
	await page.getByLabel('Jeton propriétaire').fill(process.env.E2E_OWNER_SECRET ?? '');
	await page.getByRole('button', { name: 'Entrer' }).click();
	await expect(page.getByRole('heading', { name: 'Atlas' })).toBeVisible();
}

async function shot(page: Page, name: string) {
	if (SHOTS) await page.screenshot({ path: `${SHOTS}/${test.info().project.name}-${name}.png` });
}

test('a wrong owner token is refused with an explanation', async ({ page }) => {
	await page.goto('/login');
	await page.getByLabel('Jeton propriétaire').fill('not-the-token');
	await page.getByRole('button', { name: 'Entrer' }).click();
	await expect(page.getByRole('alert')).toContainText('refusé');
	await page.goto('/');
	await expect(page).toHaveURL(/\/login$/);
});

test('the browser never receives the owner token', async ({ page, context }) => {
	await login(page);
	const cookie = (await context.cookies()).find((c) => c.name === 'nostos_owner');
	expect(cookie?.httpOnly).toBe(true);
	expect(await page.evaluate(() => document.cookie)).not.toContain('nostos_owner');
	const html = await page.content();
	expect(html).not.toContain(process.env.E2E_OWNER_SECRET ?? 'x');
});

test('atlas → trip → stop → gallery and back', async ({ page }) => {
	const errors: string[] = [];
	page.on('pageerror', (e) => errors.push(e.message));
	await login(page);

	const canvas = page.locator('canvas.maplibregl-canvas');
	await expect(canvas).toBeVisible();
	const trips = page.getByRole('region', { name: 'Odyssées' }).getByRole('button', { name: /Démo/ });
	await expect(trips).toHaveCount(2);
	await expect(page.locator('.labels .pin.trip[data-off="0"]').first()).toBeVisible();
	await page.waitForTimeout(3000);
	await shot(page, '1-atlas');

	await trips.filter({ hasText: "L'été indien" }).click();
	await expect(page.getByRole('heading', { name: "Démo · L'été indien" })).toBeVisible();
	await expect(page.getByText(/5 escales · 18 vues · \d[\d\s ]* km/)).toBeVisible();
	const stops = page.getByLabel('Escales', { exact: true }).getByRole('button');
	await expect(stops).toHaveCount(5);
	await expect(page).toHaveURL(/\?trip=quebec-2026$/);
	await page.waitForTimeout(2500);
	await shot(page, '2-trip');

	await stops.nth(2).click();
	const gallery = page.getByRole('complementary');
	await expect(gallery.getByRole('heading', { level: 2 })).toHaveText('Chutes Montmorency');
	await expect(gallery).toContainText('Chant IV');
	await expect(gallery).toContainText('46.89');
	const lead = gallery.locator('figure img');
	await expect(lead).toBeVisible();
	await expect.poll(() => lead.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBeGreaterThan(1000);
	await expect(gallery.getByRole('region', { name: 'Recette' })).toContainText('Ombres');
	await expect(gallery.locator('figcaption')).toContainText('1 ▸ 4 · Classic Chrome');
	await page.keyboard.press('ArrowRight');
	await expect(gallery.locator('figcaption')).toContainText('2 ▸ 4');
	await expect(page).toHaveURL(/stop=[0-9a-f-]{36}/);
	await page.waitForTimeout(2200);
	await shot(page, '3-gallery');

	await page.keyboard.press('Escape');
	await expect(gallery).toHaveCount(0);
	await page.keyboard.press('Escape');
	await expect(page.getByRole('heading', { name: 'Atlas' })).toBeVisible();
	expect(errors).toEqual([]);
});

test('a shared link opens the same stop', async ({ page }) => {
	await login(page);
	await page.getByRole('button', { name: /Lisbonne/ }).first().click();
	await page.getByLabel('Escales', { exact: true }).getByRole('button').first().click();
	await expect(page).toHaveURL(/stop=/);
	const url = page.url();
	await page.goto('about:blank');
	await page.goto(url);
	await expect(page.getByRole('complementary').getByRole('heading', { level: 2 })).toHaveText('Alfama');
	await expect(page.getByRole('complementary')).toContainText('Reala Ace');
});
