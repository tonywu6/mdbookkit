/**
 * mdBook 0.5.2 has issues highlighting TOC if `.html` is stripped from URL.
 * See <https://github.com/rust-lang/mdBook/issues/2962>. Unfortunately `.html` stripping
 * is Cloudflare's default behavior.
 *
 * The worker will manually redirect requests to paths without `.html` back to their
 * actual file path.
 */

const main = {
  async fetch(req, env, ctx): Promise<Response> {
    const res = await env.ASSETS.fetch(req);

    switch (res.status) {
      case 307: {
        const loc = res.headers.get("location");
        if (loc === null) {
          return res;
        }

        const url = new URL(loc, res.url);
        req = new Request(url.toString(), req);
        // `req` should now return an OK response

        if (res.url === canonicalUrl(url).toString()) {
          // the requested path is already canonical
          // which means we can pass the content through
          return await env.ASSETS.fetch(req);
        } else {
          // manually following the redirect
          // this will be handled by the case below
          return await main.fetch(req, env, ctx);
        }
      }

      case 200:
      case 304: {
        if (res.headers.get("content-type")?.startsWith("text/html")) {
          // got an OK response from this request
          // ask client to retry using the completed path instead
          return new Response(null, {
            status: 307,
            headers: { location: canonicalUrl(new URL(res.url)).toString() },
          });
        } else {
          return res;
        }
      }

      default:
        return res;
    }
  },
} satisfies ExportedHandler<Env>;

function canonicalUrl(url: URL): URL {
  url = new URL(url);
  if (url.pathname.endsWith("/")) {
    url.pathname = `${url.pathname}index.html`;
  } else {
    url.pathname = `${url.pathname}.html`;
  }
  return url;
}

export default main;
