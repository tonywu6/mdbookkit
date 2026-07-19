const main = {
  async fetch(req, env, ctx): Promise<Response> {
    // TODO: redirects
    return await env.ASSETS.fetch(req);
  },
} satisfies ExportedHandler<Env>;

export default main;
