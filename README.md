# wasi-replay

WASI Preview 2 の WIT に書かれた型情報を使って、
Wasm コンポーネントの実行を記録・再生する実験。

## いまできること

- `wasi:random` と `wasi:clocks` のホスト呼び出しを記録する
- 記録した台帳から、同じ実行を何度でも再生する
- 記録時と再生時で呼び出しの順序や回数がズレたら止める

```bash
cargo run -p host -- record
cargo run -p host -- replay
```

## 動作環境

- Rust 1.94
- wasmtime 47.0.3
- cargo-component 0.21.1
- wasm-tools 1.256.0
- WASI 0.2（Preview 2）