const main = {
  async fetch(req, env, ctx): Promise<Response> {
    const res = await env.ASSETS.fetch(req);
    switch (res.status) {
      case 404:
        if (req.headers.get("sec-fetch-mode") === "navigate") {
          const path = new URL(req.url).pathname
            .replace(/^\/mdbookkit/, "")
            .replace(/\.html$/, "")
            .replace(/\/$/, "");
          const location = {
            "/permalinks/configuration": "/permalinks/reference/configuration",
            "/permalinks/continuous-integration":
              "/permalinks/how-to/continuous-integration",
            "/permalinks/features": "/permalinks/getting-started",
            "/permalinks/logging": "/permalinks/reference/environment-variables",
            "/permalinks/more-ways-to-link": "/permalinks/how-to/hardcoded-links",
            "/rustdoc-links/configuration": "/rustdoc-links/reference/configuration",
            "/rustdoc-links/continuous-integration":
              "/rustdoc-links/how-to/continuous-integration",
            "/rustdoc-links/known-issues": "/rustdoc-links/faq",
            "/rustdoc-links/logging": "/rustdoc-links/reference/environment-variables",
            "/rustdoc-links/name-resolution": "/rustdoc-links/naming-items",
            "/rustdoc-links/supported-syntax": "/rustdoc-links/writing-links",
            "/rustdoc-links/workspace-layout": "/rustdoc-links/how-to/cargo-workspaces",
          }[path];
          if (location !== undefined) {
            const url = new URL(req.url);
            url.pathname = `/mdbookkit${location}`;
            return Response.redirect(url.href, 308);
          } else {
            return res;
          }
        } else {
          return res;
        }
        break;
      default:
        return res;
    }
  },
} satisfies ExportedHandler<Env>;

export default main;
