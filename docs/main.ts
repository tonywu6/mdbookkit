const main = {
  async fetch(req, env, ctx): Promise<Response> {
    return await env.ASSETS.fetch(req);
  },
} satisfies ExportedHandler<Env>;

export default main;
