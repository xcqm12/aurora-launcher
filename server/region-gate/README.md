# Aurora Region Gate (Cloudflare Worker)

GitHub Releases cannot restrict downloads by country. This Worker is a
server-side gate: it reads `request.cf.country`, rejects configured countries,
and streams the asset from `ORIGIN_BASE`.

## Deploy

1. Upload the installers to an origin you control (R2 public bucket, S3, etc.).
2. Set `ORIGIN_BASE` and `BLOCKED_COUNTRIES` in `wrangler.toml`.
3. `npx wrangler deploy`.
4. Publish the Worker URLs (not the raw GitHub asset URLs) as the restricted
   download links.

## Limits

- Only traffic that goes through the Worker is gated. GitHub Release assets
  remain public, so do not attach restricted binaries to the public release if
  you need real enforcement.
- VPN/proxy users can bypass country detection. This is a distribution control,
  not a security boundary.
- The launcher's built-in `cn-restricted` check is a UX hint only.
