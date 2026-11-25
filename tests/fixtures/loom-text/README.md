# Weighted text CPU fixture

Test data from [ROCm/hrx-system v0.3.0, `loom/src/loom/serve/fixture/text`](https://github.com/ROCm/hrx-system/tree/v0.3.0/loom/src/loom/serve/fixture/text), under the included upstream `LICENSE`. This is a tiny deterministic test model, not a trained assistant.

`model.json` is upstream `native.card.json`. Included are the programs and tokenizer used by its `check` preset with the source variant. `metadata.safetensors` (key `unused`, value 0) and `bias.data` (key `bias`, value 2) are the scalar checkpoint shards generated according to the upstream fixture BUILD rules. Other presets and archive variants are not part of this test. No Python or network fetch is required to use this fixture.
