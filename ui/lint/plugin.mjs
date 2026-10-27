// The UI's own oxlint rules. `.oxlintrc.json` runs the ones the code already
// meets; `ratchet.json` holds the rest, whose counts scripts/ci/check_ui_ratchet.py
// lets only fall.

const KIND_WORDS = new Set(["Movie", "Series", "Season", "Episode", "movie", "tv", "series", "episode"])

const PALETTE_COLOR = new RegExp(
  String.raw`(?<![\w-])(?:bg|text|border(?:-[trblxyse])?|from|via|to|ring|ring-offset|fill|stroke|shadow|outline|divide|placeholder|accent|caret|decoration)-`
    + String.raw`(?:(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d{2,3}|black|white)`
    + String.raw`(?![\w-])`,
  "g",
)

const isKindWord = (node) => node.type === "Literal" && KIND_WORDS.has(node.value)

/** Whether an expression reads a query result's `data`, as in `items.data?.list`. */
function readsData(node) {
  for (let current = node; ; ) {
    if (current.type === "ChainExpression") current = current.expression
    else if (current.type === "MemberExpression") {
      if (!current.computed && current.property.name === "data") return true
      current = current.object
    } else return current.type === "Identifier" && current.name === "data"
  }
}

const isEmptyLiteral = (node) =>
  (node.type === "ArrayExpression" && node.elements.length === 0)
  || (node.type === "ObjectExpression" && node.properties.length === 0)

function reportPaletteColors(context, node, text) {
  for (const [color] of text.matchAll(PALETTE_COLOR)) {
    context.report({ node, message: `\`${color}\` is a raw palette color; use a theme token from app.css.` })
  }
}

export default {
  meta: { name: "ui" },
  rules: {
    "no-inline-kind-translation": {
      create(context) {
        return {
          ConditionalExpression(node) {
            const { test, consequent, alternate } = node
            if (test.type !== "BinaryExpression" || !["===", "!==", "==", "!="].includes(test.operator)) return
            if (!isKindWord(test.left) && !isKindWord(test.right)) return
            if (!isKindWord(consequent) || !isKindWord(alternate)) return
            context.report({ node, message: "Translate media kinds through `@/lib/kinds`, not an inline ternary." })
          },
        }
      },
    },
    "no-query-data-empty-fallback": {
      create(context) {
        return {
          LogicalExpression(node) {
            if (node.operator !== "??" || !isEmptyLiteral(node.right) || !readsData(node.left)) return
            context.report({
              node,
              message: "An empty fallback for query `data` hides loading and errors; render those states instead.",
            })
          },
        }
      },
    },
    "no-palette-color": {
      create(context) {
        return {
          Literal(node) {
            if (typeof node.value === "string") reportPaletteColors(context, node, node.value)
          },
          TemplateElement(node) {
            reportPaletteColors(context, node, node.value.raw)
          },
        }
      },
    },
  },
}
