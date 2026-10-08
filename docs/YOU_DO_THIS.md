# You do this

One-time steps only you can do. Everything else runs in the cloud session and GitHub Actions. Nothing installs on your PC.

## 1. Free Cloudflare account (needed to put the game online)

No credit card. The game builds and tests without this. CI skips the deploy step until steps 1 to 3 are done.

Note: the cloud session cannot open Cloudflare's docs site, so I could not check these click paths against it. Cloudflare sometimes renames menu items. If a name below does not match, look for the closest one.

1. Go to https://dash.cloudflare.com/sign-up and sign up with email and password. Skip any paid plan offer. Do not add a card.
2. Confirm your email from the message Cloudflare sends.

## 2. Make an API token

1. Go to https://dash.cloudflare.com/profile/api-tokens
2. Click **Create Token**.
3. Next to **Edit Cloudflare Workers**, click **Use template**.
4. Under **Permissions**, click **Add more** and add: **Account**, **Cloudflare Pages**, **Edit**.
5. Under **Account Resources**, pick your account. Under **Zone Resources**, pick **All zones**.
6. Click **Continue to summary**, then **Create Token**.
7. Copy the token. Cloudflare shows it only once.

## 3. Put two secrets into GitHub

1. Go to https://github.com/dew0001/last-call/settings/secrets/actions
2. Click **New repository secret**. Name: `CLOUDFLARE_API_TOKEN`. Value: the token from step 2. Click **Add secret**.
3. Find your account ID: go to https://dash.cloudflare.com, click **Workers & Pages** in the left menu. The **Account ID** is in the right column. Copy it.
4. Click **New repository secret** again. Name: `CLOUDFLARE_ACCOUNT_ID`. Value: the account ID. Click **Add secret**.

The next push to `main` deploys the game to `https://last-call.pages.dev` (or the closest free name) and the signaling Worker to `*.workers.dev`.

## 4. Optional: let the cloud session download Firefox and WebKit

The cloud session's network policy blocks the Playwright browser download hosts. Firefox and WebKit tests still run in GitHub Actions, so this is optional.

To allow them: open the cloud environment menu in the session's title bar, click **Edit**, and go to **Network access**. Under **Allowed domains**, add `cdn.playwright.dev` and `playwright.download.prss.microsoft.com`. Leave **Allow package managers** ticked. Steps: https://code.claude.com/docs/en/cloud-environments#network-access

The same blocked list includes `developers.cloudflare.com`. Adding it lets the cloud session check the Cloudflare steps above.
