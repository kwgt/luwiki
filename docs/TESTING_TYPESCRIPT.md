# TypeScript テスト方針

本書は LuWiki における TypeScript テストコードの
基本方針を整理するための仮文書である。

本書では主に以下を定義対象とする。

- TypeScript テストコードの対象範囲
- テスト基盤の選定方針
- 型チェックおよび静的解析の扱い
- `tsconfig` 構成方針
- テストコードの import と型定義の扱い

本書は `frontend` の lint / test 整理タスクで確定した
TypeScript test 方針を整理した文書である。

---

## 1. 目的

TypeScript による front-end test を、
実装コードと同様に保守対象として扱える状態を維持する。

本方針の主目的は以下の通り。

- front-end test の型安全性を維持する
- 実装コードと test コードの責務差を `tsconfig` で適切に分離する
- lint 時に test コード起因の不整合を早期検出できるようにする
- 不要な test ランナー依存を増やさず、既存構成に整合させる

---

## 2. 対象範囲

### 2.1 対象ディレクトリ

- 本書は主として `frontend` 配下の TypeScript test を対象とする
- 現時点では `frontend/src/**/*.test.ts` を主対象とする

### 2.2 対象コード

- ユーティリティ関数の test
- エディタ補完や front matter まわりの補助ロジックの test
- DOM モックを用いる軽量 test

### 2.3 現時点で対象外のもの

- Rust 側 test
- REST API 結合 test
- ブラウザ E2E test
- 将来的に別ランナーを要する統合 test

---

## 3. 基本方針

### 3.1 test コードも保守対象とする

- `*.test.ts` を一時的な補助コードとして扱わない
- 実装コードと同様に型エラーを放置しない
- lint / 型チェック対象から外すことで不整合を隠蔽しない

### 3.2 lint 対象に含める

- `frontend` における TypeScript test は lint / 型チェック対象に含める
- test コードの型不整合も、M1 以降の機能保守に影響する不具合候補として扱う

### 3.3 実装コードと test コードの設定責務は分離する

- app 本体用の TypeScript 設定と test 用の TypeScript 設定は分離してよい
- ただし、`npm run lint` などの開発導線全体では両者を検査対象に含める

---

## 4. テスト基盤方針

### 4.1 標準 test 基盤

- front-end の TypeScript test では `node:test` を標準とする
- assertion には `node:assert/strict` を標準とする

### 4.2 採用理由

- 既存 test 実装がすでに `node:test` / `node:assert/strict` を使用している
- 現時点の `frontend/package.json` には Vitest / Jest / Mocha 等の
  専用 test ランナー依存や script が存在しない
- 追加依存を増やさず、現行環境で最小変更の運用ができる

### 4.3 今後の扱い

- 新規 TypeScript test を追加する場合も、第一候補は `node:test` とする
- 別ランナーを導入する場合は、その必要性と既存方針との差分を明示した上で
  本書を更新する

---

## 5. 型チェック方針

### 5.1 test コードを型チェック対象に含める

- `frontend/src/**/*.test.ts` を型チェック対象に含める
- test を除外して lint を通す対応は原則として採らない

### 5.2 `tsconfig` の分離

- app 本体向け `tsconfig` と test 向け `tsconfig` は分離する方針とする
- test 用 `tsconfig` では `node:test` / `node:assert/strict` を解決できるよう
  Node 系型定義を扱う
- app 本体向け `tsconfig` には、不要な Node 系前提を持ち込まない
- 現在の `frontend` では以下の構成を採用する
  - `tsconfig.base.json`: 共通 strict 設定
  - `tsconfig.json`: app 本体用。`src/**/*.vue` と `src/**/*.ts` を対象とし、`src/**/*.test.ts` は除外する
  - `tsconfig.test.json`: test 用。`src/**/*.test.ts` と `src/**/*.d.ts` を対象とし、`types: [\"node\"]` を有効にする

### 5.3 lint 導線

- `npm run lint` は app 本体用と test 用の両方を検査できる構成を目指す
- 単一の `tsconfig` に責務を押し込めるのではなく、
  役割ごとに設定を分けた上で lint 導線を統一する
- 現在の `frontend/package.json` では
  `tsc -p tsconfig.json && tsc -p tsconfig.test.json`
  を `lint` script として採用する

---

## 6. import / 型定義方針

### 6.1 import 記法

- test コードの import 記法は、採用した `tsconfig` と整合する形に揃える
- `.ts` 拡張子付き import を使用する場合は、
  TypeScript 設定側で明示的に整合が取れていることを前提とする
- 現在の `frontend/src/**/*.test.ts` では、
  相対 import は `.ts` 拡張子を付けない記法へ統一する

### 6.2 外部ライブラリ型定義

- 外部ライブラリに型定義が不足する場合は、以下の順で検討する
  - 既存の型定義パッケージ導入
  - ローカル宣言ファイルによる補完
  - 依存側利用方法の見直し
- 現在の `frontend` では次を採用している
  - `@types/node`: `node:test` / `node:assert/strict` の解決用
  - `@types/d3`: `mermaid` 経由の `d3` 型不足解消用
  - `@types/prismjs`: `pageCommon.ts` の `Prism` 利用解決用
  - `src/types/markdown-it-task-lists.d.ts`: `markdown-it-task-lists` のローカル宣言

### 6.3 `any` の扱い

- test コードでも `any` の安易な導入は避ける
- 不定値を扱う必要がある場合は `unknown` と型ガードを優先する

---

## 7. front matter 関連 test の扱い

- front matter 補完 source は `CompletionResult | Promise<CompletionResult | null> | null`
  を返しうるため、test 側では同期前提で扱わず `await` で正規化する
- `result.options` を参照する test では、`result` の null を先に排除する
- callback 引数の暗黙 `any` は避け、`Completion` などの型を明示する
- front matter 付き Markdown に対しても、本文見出し解析、wiki link 解析、
  mermaid 補完、front matter 補完が維持されることを回帰確認対象とする

---

## 8. 今後の更新対象

以下は今後の lint / test 整理タスクで追記・更新の可能性がある。

- `package.json` における `test` script の正式形
- DOM モックの共通化方針
- test 補助用 `.d.ts` の配置方針
- 将来的に別ランナーを導入する場合の移行条件
