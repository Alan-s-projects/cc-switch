# Bundled Model Pricing

Atlas bundles 219 model-price entries in
`src-tauri/src/resources/model-pricing.json`. The file is compiled into the app;
it is not downloaded or synchronized at runtime.

## Provenance

- Source repository: [farion1231/cc-switch](https://github.com/farion1231/cc-switch).
- Revision: `1ee2fdc3a791f1e73476c631c7ab7ce8fac0638f`.
- Source file: `src-tauri/src/database/schema.rs`, `seed_model_pricing`.
- Source blob: `cb4724133064a52b006f674da0f958d590c068fe`.
- Source license: MIT.

The six fields in each row are model ID, display name, input, output, cache read,
and cache creation. All four prices are decimal USD per one million tokens.
The snapshot was extracted from Rust string literals with a Rust syntax parser,
not inferred from model names or generated from a model-family multiplier.

## Behavior

Startup fills missing defaults without overwriting saved prices. Local overrides
and deletion tombstones are stored in Atlas's `model-pricing.json` data file.
These apply to every vendor and to future custom model IDs.

The Cost Pricing reset action clears overrides and tombstones for all models,
then restores the compiled defaults. Models absent from the bundled snapshot
become unpriced. The confirmation describes this scope. Existing nonzero request
costs, imported conversation history, and retired metadata are not repriced.

New models remain usable without a known price. Usage displays a missing-price
warning instead of borrowing a price from a different model or vendor. The
current snapshot does not price `mai-code-1.1-flash` or the internal
`gpt-5.6-sol-fast` entry.

Price lookup tries the exact model ID first. Legacy OpenAI GPT namespace/effort
aliases and dated variants are supported; arbitrary namespaces and future
vendor-specific suffixes are not treated as interchangeable SKUs.

## Limitations

These are the flat token estimates recorded by cc-switch, not a GitHub Copilot
subscription bill or independently verified live vendor rates. The source table
does not represent every long-context tier, region, promotion, batch mode,
tool-call fee, image/audio rate, or time-based cache-storage charge. For example,
the Gemini 3.6-3.8 Flash entries use cc-switch's introductory rates, and Grok 4.5-4.7
entries use its base context tier.

Future pricing updates require a reviewed bundled-data update or a manual price
override. A newly discovered model is not assumed to be free just because its
price has not yet been added.
