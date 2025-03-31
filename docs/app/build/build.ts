import { encodeHex } from "jsr:@std/encoding/hex";
import * as pathlib from "jsr:@std/path";
import * as esbuild from "npm:esbuild";

const relpath = (path: string) => new URL(path, import.meta.url).pathname;

const built = await esbuild.build({
  bundle: true,
  target: ["chrome93", "firefox93", "safari15", "es2020"],
  platform: "browser",
  plugins: [remoteCSS()],
  entryPoints: [relpath("../main.css")],
  outdir: relpath("../../src/app"),
  entryNames: "[name]-[hash]",
  metafile: true,
});

// generate an `app/dist.css` for mdBook that actually imports the bundle

await Deno.writeTextFile(
  relpath("../dist.css"),
  Object.entries(built.metafile.outputs)
    .filter(([, file]) => file.entryPoint)
    .map(([path]) => `@import url(${JSON.stringify(pathlib.basename(path))});`)
    .join("\n"),
);

function remoteCSS(): esbuild.Plugin {
  return {
    name: "remote-css",
    setup: (build) => {
      const cacheDir = relpath("../../build/css");

      build.onResolve(
        {
          filter: /^https:\/\//,
          namespace: "file",
        },
        ({ path, kind }) =>
          kind === "import-rule" ? { path, namespace: "remote-css" } : undefined,
      );

      build.onResolve(
        {
          filter: /^https:\/\//,
          namespace: "remote-css",
        },
        ({ path, kind }) =>
          kind === "url-token" ? { path, namespace: "remote-url" } : undefined,
      );

      build.onLoad(
        {
          filter: /.*/,
          namespace: "remote-css",
        },
        async ({ path }) => {
          const result = await cachedFetch(path, {
            headers: {
              "user-agent":
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:137.0) Gecko/20100101 Firefox/93.0",
            },
          })
            .then((res) => new TextDecoder().decode(res))
            .then((text) => ({ type: "ok", text }) as const)
            .catch((err) => ({ type: "err", err }) as const);

          if (result.type === "err") {
            return {
              path,
              external: true,
              warnings: [{ text: `failed to download ${path}: ${result.err}` }],
            };
          }

          return { contents: result.text, loader: "css" };
        },
      );

      build.onLoad(
        {
          filter: /.*/,
          namespace: "remote-url",
        },
        async ({ path }) => {
          const result = await cachedFetch(path)
            .then((blob) => ({ type: "ok", blob }) as const)
            .catch((err) => ({ type: "err", err }) as const);

          if (result.type === "err") {
            return {
              path,
              external: true,
              warnings: [{ text: `failed to download ${path}: ${result.err}` }],
            };
          }

          return { contents: result.blob, loader: "copy" };
        },
      );

      async function cachedFetch(...[input, init]: Parameters<typeof fetch>) {
        const key = await crypto.subtle
          .digest("SHA-256", new TextEncoder().encode(JSON.stringify({ input, init })))
          .then(encodeHex);

        const cachePath = pathlib.join(cacheDir, key);

        try {
          return await Deno.readFile(cachePath);
        } catch {
          // ignored
        }

        const contents = await fetch(input, init)
          .then((res) => {
            if (res.status >= 400) {
              throw new Error(res.statusText);
            } else {
              return res;
            }
          })
          .then((res) => res.bytes());

        await Deno.mkdir(cacheDir, { recursive: true });
        await Deno.writeFile(cachePath, contents);

        return contents;
      }
    },
  };
}
