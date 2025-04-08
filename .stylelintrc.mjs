// @ts-check

/** @type {import('stylelint').Config} */
export default {
  extends: ["stylelint-config-standard", "stylelint-config-recess-order"],
  rules: {
    "no-descending-specificity": null, // unactionable with nested CSS
  },
};
