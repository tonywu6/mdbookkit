import * as esbuild from "npm:esbuild";

const relpath = (path: string) => new URL(path, import.meta.url).pathname;

await esbuild.build({
  entryPoints: [relpath("../index.css")],
  outdir: relpath("../../theme"),
  bundle: true,
  target: ["chrome93", "firefox93", "safari15", "es2020"],
  platform: "browser",
});
