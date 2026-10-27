/**
 * Region gate for Aurora Launcher release assets.
 *
 * This Worker is the only supported way to enforce a region restriction:
 * GitHub Releases cannot block downloads by country. Point ORIGIN_BASE at
 * an R2 bucket or another origin you control, and hand out Worker URLs
 * instead of raw GitHub asset URLs.
 */
export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/health") {
      return new Response("ok", { headers: { "content-type": "text/plain" } });
    }

    const country = (request.cf && request.cf.country) || "";
    const blocked = (env.BLOCKED_COUNTRIES || "CN")
      .split(",")
      .map((value) => value.trim().toUpperCase())
      .filter(Boolean);
    if (blocked.includes(country)) {
      return new Response("This build is not available in your region.", {
        status: 403,
        headers: { "content-type": "text/plain; charset=utf-8" },
      });
    }

    if (!env.ORIGIN_BASE) {
      return new Response("ORIGIN_BASE is not configured.", { status: 500 });
    }
    const prefix = env.PATH_PREFIX || "/aurora";
    let pathname = url.pathname;
    if (prefix && pathname.startsWith(prefix)) {
      pathname = pathname.slice(prefix.length) || "/";
    }
    const origin = new URL(env.ORIGIN_BASE);
    const target = new URL(pathname + url.search, origin);
    const headers = new Headers(request.headers);
    headers.delete("host");
    const response = await fetch(target, {
      method: request.method,
      headers,
      body: request.method === "GET" || request.method === "HEAD" ? undefined : request.body,
      redirect: "follow",
    });
    const output = new Headers(response.headers);
    output.set("x-aurora-region", country || "unknown");
    return new Response(response.body, { status: response.status, headers: output });
  },
};
