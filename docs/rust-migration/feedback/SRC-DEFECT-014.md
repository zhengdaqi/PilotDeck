# SRC-DEFECT-014: `Object.prototype` names are accepted as gray-matter engines

- Source: gray-matter looks engines up with `opts.engines[name]` on a plain object. A front matter that opens with `---toString` (or `---constructor`, `---valueOf`, ...) therefore runs `Object.prototype.toString` and similar functions as the parser:
  - `---toString` gives data `"[object Object]"`;
  - `---__proto__` throws a TypeError.
- Fix: only real engine names are recognised (yaml/yml, plus json/js/javascript, which frontmatter.js disables). Any other name gives gray-matter's own error, `gray-matter engine "<name>" is not registered`.
- Evidence: `frontmatter_oracle` cases that open with `---toString`.
