import { encodeHex } from "@std/encoding/hex";
import * as pathlib from "@std/path";
import * as esbuild from "esbuild";
import fs from "node:fs/promises";
import process from "node:process";

const relpath = (path: string) => new URL(path, import.meta.url).pathname;

await fs.rm(relpath("../src/web"), { recursive: true, force: true });

const built = await esbuild.build({
  bundle: true,
  splitting: true,
  format: "esm",
  target: ["chrome93", "firefox93", "safari15", "es2020"],
  platform: "browser",
  plugins: [remoteCSS()],
  entryPoints: [relpath("../web/main.ts"), relpath("../web/main.css")],
  outdir: relpath("../src/web"),
  entryNames: "[name]-[hash]",
  metafile: true,
  logLevel: "CI" in process.env ? "info" : undefined,
});

const css: string[] = [];
const esm: string[] = [];

for (const [path, file] of Object.entries(built.metafile.outputs)) {
  if (file.entryPoint?.startsWith("web/")) {
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

await fs.writeFile(relpath("../web/loader.css"), css.join("\n") + "\n", "utf-8");

await fs.writeFile(
  relpath("../web/loader.js"),
  `document.addEventListener("DOMContentLoaded", () => { ${esm.join("\n")} });`,
  "utf-8",
);

function remoteCSS(): esbuild.Plugin {
  return {
    name: "remote-css",
    setup: (build) => {
      const cacheDir = relpath("../build/css");

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
          return await fs.readFile(cachePath);
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

        await fs.mkdir(cacheDir, { recursive: true });
        await fs.writeFile(cachePath, contents, "utf-8");

        return contents;
      }
    },
  };
}
