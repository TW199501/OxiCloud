import { test as base, Page, expect } from '@playwright/test';

/**
 * Extended `test` fixture that fails on any unhandled browser-side JavaScript
 * error (SyntaxError, ReferenceError, uncaught promise rejections, etc.).
 *
 * Import `test` from this module instead of `@playwright/test` so every spec
 * gets the listener automatically without per-file boilerplate.
 */
export const test = base.extend<object>({
    page: async ({ page }, use) => {
        const jsErrors: Error[] = [];
        page.on('pageerror', (err) => jsErrors.push(err));
        await use(page);
        if (jsErrors.length > 0) {
            throw new Error(
                `${jsErrors.length} unhandled JS error(s) on page:\n` +
                jsErrors.map((e) => `  • ${e.message}`).join('\n')
            );
        }
    },
});

export const TEST_ADMIN = {
  username: 'admin',
  email: 'testadmin@example.com',
  password: 'TestPassword1!',
};

/**
 * Log in as the test admin and wait until the main app is fully initialized.
 *
 * We wait for two things after the login redirect:
 *  1. `#sidebar` — confirms the main HTML has loaded.
 *  2. `#user-avatar-btn .user-vignette` — confirms that `setupUserMenu()` has
 *     run and mounted the avatar vignette.  This is the earliest reliable
 *     signal that the click-handler on the avatar button is attached, so any
 *     subsequent test that opens the user menu will not race against JS startup.
 *
 * Without (2), CI (Ubuntu + Xvfb) occasionally clicks the button before the
 * event listener is registered because the JS runtime is slower than on macOS.
 */
export async function loginAsAdmin(page: Page) {
  await goToLoginPage(page);
  await page.locator('#login-username').fill(TEST_ADMIN.username);
  await page.locator('#login-password').fill(TEST_ADMIN.password);
  await page.locator('#login-panel button[type="submit"]').click();
  await expect(page.locator('#sidebar')).toBeVisible({ timeout: 15_000 });
  // Wait for the JS app to initialise: avatar vignette present ⟹ click handler attached.
  await expect(page.locator('#user-avatar-btn .user-vignette')).toBeAttached({ timeout: 10_000 });
}

/**
 * Navigate to `/` and land on the login panel, handling the language selector
 * if it appears (fresh localStorage). The admin account is guaranteed to exist
 * because globalSetup created it before any test ran.
 */
export async function goToLoginPage(page: Page) {
  await page.goto('/');

  // Both panels start with .hidden — wait for JS to reveal one.
  // Use expect() (5 s default) rather than waitForSelector() (30 s) so a JS
  // crash fails fast instead of hanging for the full test timeout.
  await expect(
    page.locator('#language-panel:not(.hidden), #login-panel:not(.hidden)').first()
  ).toBeAttached();

  if (await page.locator('#language-panel').isVisible()) {
    await page.locator('#language-continue').click();
  }

  await expect(page.locator('#login-panel')).toBeVisible();
}
