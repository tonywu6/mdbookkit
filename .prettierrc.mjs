// @ts-check

/** @type {import("prettier").Config} */
export default {
  printWidth: 88,
  proseWrap: "always",
  tabWidth: 2,
  useTabs: false,
  overrides: [
    {
      files: [".github/workflows/**/*.yml"],
      options: {
        proseWrap: "preserve",
      },
    },
  ],
};
