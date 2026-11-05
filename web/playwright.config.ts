import { defineConfig, devices } from '@playwright/test';

// Run through ../scripts/e2e.sh, which starts the API, seeds demo data and
// serves the production build, then sets E2E_BASE_URL and E2E_OWNER_SECRET.
export default defineConfig({
	testDir: 'tests',
	timeout: 60_000,
	expect: { timeout: 15_000 },
	retries: 0,
	reporter: [['list']],
	use: {
		baseURL: process.env.E2E_BASE_URL ?? 'http://127.0.0.1:4173',
		launchOptions: { args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] }
	},
	projects: [
		{ name: 'desktop', use: { viewport: { width: 1440, height: 900 } } },
		{ name: 'mobile', use: { ...devices['Pixel 7'] } }
	]
});
