# Aurora Region Gate (Cloudflare Worker)

GitHub Releases cannot restrict downloads by country. This Worker is a
server-side gate: it reads `request.cf.country`, rejects configured countries,
and streams the asset from `ORIGIN_BASE`.

## Deploy

1. Set `ORIGIN_BASE` and `BLOCKED_COUNTRIES` in `wrangler.toml`. The shipped
   config proxies `qlmtest.dpdns.org/aurora/*` to the v0.4.6 GitHub Release
   assets, so no separate origin is required.
2. Authenticate wrangler with a **scoped API token** (Workers Scripts:Edit,
   Zone Workers Routes:Edit, DNS:Edit, Zone:Read for the zone) — a Global API
   Key also needs `CLOUDFLARE_EMAIL` and grants full account access, so it is
   not recommended. Then run `npx wrangler deploy`.
3. Publish `https://qlmtest.dpdns.org/aurora/<asset>` URLs (not the raw GitHub
   asset URLs) as the restricted download links.

Local verification without an account:

```bash
node --input-type=module -e "import('./src/index.js').then(async ({default: worker}) => { let target=''; globalThis.fetch = async (url) => { target = url; return new Response('asset', {status:200}); }; const env = { BLOCKED_COUNTRIES: 'CN', PATH_PREFIX: '/aurora', ORIGIN_BASE: 'https://github.com/xcqm12/aurora-launcher/releases/download/v0.4.6/' }; const req = (c) => ({url:'https://qlmtest.dpdns.org/aurora/Aurora-Launcher-0.4.6-standard-x64-setup.exe',method:'GET',headers:new Headers(),body:null,cf:{country:c}}); console.log('CN', (await worker.fetch(req('CN'), env)).status); console.log('SG', (await worker.fetch(req('SG'), env)).status, target); });"
```

Expected: CN returns 403, SG returns 200 and the fetch target is the release
asset URL.

## Limits

- Only traffic that goes through the Worker is gated. GitHub Release assets
  remain public, so do not attach restricted binaries to the public release if
  you need real enforcement.
- VPN/proxy users can bypass country detection. This is a distribution control,
  not a security boundary.
- The launcher's built-in `cn-restricted` check is a UX hint only.
