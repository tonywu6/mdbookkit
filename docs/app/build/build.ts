import { encodeHex } from "jsr:@std/encoding/hex";
import * as pathlib from "jsr:@std/path";
import * as esbuild from "npm:esbuild";

const relpath = (path: string) => new URL(path, import.meta.url).pathname;

try {
  await Deno.remove(relpath("../../src/app"), { recursive: true });
} catch {
  // recursive: true is supposed to make it not throw ...
}

const built = await esbuild.build({
  bundle: true,
  format: "esm",
  target: ["chrome93", "firefox93", "safari15", "es2020"],
  platform: "browser",
  plugins: [remoteCSS()],
  entryPoints: [relpath("../main.js"), relpath("../main.css")],
  outdir: relpath("../../src/app"),
  entryNames: "[name]-[hash]",
  metafile: true,
  logLevel: Deno.env.get("CI") ? "info" : undefined,
});

// generate `app/dist.css` and `app/dist.js` for mdBook that actually import the bundle

const css: string[] = [];
const esm: string[] = [];

for (const [path, file] of Object.entries(built.metafile.outputs)) {
  if (file.entryPoint) {
    const name = JSON.stringify(`./${pathlib.basename(path)}`);
    switch (path.split(".").pop()) {
      case "css":
        css.push(`@import url(${name});`);
        break;
      case "js":
        esm.push(`import(${name});`);
        break;
      default:
        break;
    }
  }
}

await Deno.writeTextFile(relpath("../dist.css"), css.join("\n") + "\n");

await Deno.writeTextFile(
  relpath("../dist.js"),
  `document.addEventListener("DOMContentLoaded", () => { ${esm.join("\n")} });`,
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
        ({ path, kind }) => {
          if (kind !== "url-token") {
            return undefined;
          }
          if (!pathlib.extname(path)) {
            path += "#.bin";
          }
          return { path, namespace: "remote-url" };
        },
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
