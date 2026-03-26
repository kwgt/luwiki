# MCP関連 CLI拡張 テスト観点一覧

本書は、MCP実装に伴って拡張される CLI のテスト観点を整理した独立したテスト観点書である。

位置付けとしては、`docs/MCP_IMPLEMENTATION_DESIGN_TASKS.md` の 4.9.4 に対応する成果物であり、
MCP 設計書群から参照される実装・テスト実装向け文書として扱う。

対象は主として `token create` / `token list` / `token info`、
`user add` / `user edit` / `user info` の拡張、および分解スコープ表示、
path 制約表示、`NoBasicAuth` 属性表示と入力制約である。

## 1. 参照仕様

- `docs/CLI_SPECS.md`
  - `token create`
  - `token list`
  - `token info`
  - `user add`
  - `user edit`
  - `user info`
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - 2.3.20 から 2.3.43
- `docs/MCP_DESIGN_INPUT_TASKS.md`
  - 3. `token list` 表示設計のインプット整理
- `docs/BEARER_AUTH_DESIGN.md`
  - スコープ表示
  - path prefix 制約
  - ユーザ属性モデルとの責務分離

## 2. 観点の使い方

- 本書は CLI 引数検証、表示整形、既存データ読取互換、管理コマンド責務分離の観点を、単体テスト、CLI 結合テスト、DB 連動テストへ分配するための別紙である
- 表示体裁を 1 文字単位で固定するより、誤認しないこと、責務どおりに出し分けること、互換方針どおりであることを優先する
- `token list` は一覧責務、`token info` は完全表示責務、`user list` は一覧責務、`user info` は完全表示責務という分離を崩さないことを主眼に置く

## 3. `token create` の観点

### 3.1 入力受理

1. `--scope` が `read` / `write` / `create` / `update` / `append` / `delete` を受理すること
2. `--scope` がカンマ区切り複数指定を正しく解釈すること
3. `--path-prefix` が複数指定を受理すること
4. `--ttl` が既存 `30d` / `12h` / `90m` 形式を受理すること
5. `--name` が任意名として受理されること

### 3.2 バリデーション

1. 未定義スコープを拒否すること
2. `--scope` の空要素を拒否すること
3. `--scope` の重複を除去できること
4. `--path-prefix` が正規化済み絶対パス以外を拒否すること
5. `--path-prefix` 複数指定時に 1 件でも不正があれば全体をエラーにすること
6. `--ttl` の形式不正を拒否すること
7. `--ttl` に 0 以下を指定した場合にエラーになること
8. `--name` が trim 後空の場合にエラーになること
9. 存在しない `<USER-NAME>` 指定でエラーになること

### 3.3 成功表示

1. 成功時に `TOKEN ID`、`TOKEN NAME`、`USERNAME`、`SCOPES`、`PERMISSIONS`、`TTL`、`PATH PREFIXES:`、`TIMESTAMPS:`、`TOKEN VALUE:` が表示されること
2. `SCOPES` が保存値としての指定内容を表示すること
3. `PERMISSIONS` が導出値として完全名のカンマ区切りで表示されること
4. `TOKEN NAME` 未指定時に `-` が表示されること
5. トークン平文が `TOKEN VALUE:` として発行時にのみ表示されること
6. path prefix 未指定時に `PATH PREFIXES:` 配下で `- all` が表示されること
7. path prefix 未指定時に `WARNING:` 補助表示を出せること

## 4. `token list` の観点

### 4.1 基本列構成

1. 短縮表示が `SCOPE`, `PATH`, `ID`, `USER`, `NAME`, `EXPIRES` の責務で成立すること
2. `--long-info` が `CREATE`, `STATUS` を追加すること
3. `--long-info` でも `SCOPE` と `PATH` を維持すること
4. `updated_at` が一覧表示から外れていること

### 4.2 `SCOPE` 欄

1. `SCOPE` 欄が `r` / `c` / `d` / `u` / `a` の 5 文字で実効権限を表すこと
2. 権限を持たない位置が `-` で表示されること
3. `write` 保持時に `rcdua` と表示されること
4. 分解済みスコープのみ保持時に、その実効権限だけが表示されること
5. `SCOPE` 欄が保存値そのものではなく実効権限表示であることが維持されること

### 4.3 `PATH` 欄

1. path 制約なしが `*` と表示されること
2. path 制約ありが `L` と表示されること
3. `path_prefixes` 未設定時に `*` になること
4. `/` を含む場合に `*` になること
5. 一覧では詳細 prefix 群を出さず、`token info` へ責務分離されていること

### 4.4 `STATUS` 欄

1. `revoked = true` が `revoked` と表示されること
2. 未失効かつ `expire_at <= now` が `expired` と表示されること
3. 上記以外が `alive` と表示されること
4. 失効と期限切れが混同されないこと

### 4.5 フィルタ・互換

1. `[USER-NAME]` と `--user` の同時指定を拒否すること
2. `--revoked` と `--expired` 併用時に和集合で表示すること
3. 旧 `write` 保存データが `rcdua` として表示されること
4. `path_prefixes` 欠落旧データが path 制約なしとして表示されること
5. 表示列構成が変わるため完全な機械解析互換は保証しない前提と一致していること

## 5. `token info` の観点

### 5.1 導入と基本表示

1. `token info <TOKEN-ID>` が単一トークンの完全表示出口として機能すること
2. 存在する `TOKEN-ID` に対して詳細情報を大文字ラベル形式で表示できること
3. 存在しない `TOKEN-ID` に対してエラーになること

### 5.2 表示項目

1. `TOKEN ID`、`TOKEN NAME`、`USERNAME`、`STATUS`、`SCOPES`、`PERMISSIONS`、`PATH PREFIXES:`、`TTL`、`TIMESTAMPS:` を表示すること
2. `SCOPES` が保存値を表示すること
3. `PERMISSIONS` が導出値を完全名のカンマ区切りで出力すること
4. `PATH PREFIXES:` が詳細一覧のセクションで表示されること
5. 全領域アクセス可の場合に `- all` が表示されること
6. `TOKEN NAME` 未設定時に `-` を表示すること
7. トークン平文を再表示しないこと
8. ユーザ属性を表示しないこと

## 6. `user add` の観点

### 6.1 入力受理

1. `--attribute <ATTRIBUTE>` の複数指定を受理すること
2. 初期実装の属性値 `no_basic_auth` を受理すること
3. `--display-name` を従来どおり受理すること

### 6.2 パスワード要件

1. 属性未指定時にパスワード入力プロンプトを要求すること
2. `--attribute no_basic_auth` 指定時にパスワード入力を要求しないこと
3. 同一属性の重複指定を除去できること
4. 未定義属性を拒否すること

## 7. `user edit` の観点

### 7.1 属性操作

1. `--add-attribute` が属性追加として機能すること
2. `--remove-attribute` が属性削除として機能すること
3. `--clear-attributes` が全消去として機能すること
4. `clear -> remove -> add` の適用順で解釈されること
5. `--clear-attributes --add-attribute no_basic_auth` が完全置換として扱えること

### 7.2 入力制約

1. `display_name` / `password` / 属性操作のいずれもない場合にエラーになること
2. 未定義属性を拒否すること
3. 同一オプション内の重複属性を除去できること

### 7.3 `NoBasicAuth` 遷移

1. 通常ユーザへ `NoBasicAuth` を追加する場合に `--password` が不要であること
2. `NoBasicAuth` ユーザから同属性を除去する場合に `--password` 同時指定を必須とすること
3. `NoBasicAuth` ユーザのまま表示名だけを更新する場合に `--password` 不要であること
4. `NoBasicAuth` ユーザへの `--password` 単独指定を不正入力として扱うこと

## 8. `user info` の観点

### 8.1 導入と責務

1. `user info <USER-NAME>` が単一ユーザの完全表示出口として機能すること
2. `user list` が属性詳細を持たず一覧責務を維持すること
3. `user info` 新設が既存 `user list` / `user edit` 運用を破壊しないこと

### 8.2 表示項目

1. `USER ID`、`USERNAME`、`DISPLAY NAME`、`BASIC AUTH`、`ATTRIBUTES:`、`TIMESTAMPS:` を表示すること
2. 属性なしの場合に `ATTRIBUTES:` 配下で `- none` が表示されること
3. `ATTRIBUTES:` 表示で正式名称 `NoBasicAuth` を使うこと
4. `BASIC AUTH` が `ATTRIBUTES:` から導出され、`allowed` / `denied` で表示されること
5. パスワード平文、パスワードハッシュ、ソルトを表示しないこと
6. Bearer トークン情報を表示しないこと

## 9. 責務分離と互換の観点

### 9.1 token / user の責務分離

1. `token list` / `token info` がユーザ属性を表示しないこと
2. `user info` が Bearer トークン情報を表示しないこと
3. token 系表示情報が保存専用列に依存せず導出できること

### 9.2 旧データ読取互換

1. 旧 Bearer トークン管理情報の `read` / `write` スコープを読めること
2. 旧 Bearer トークン管理情報の `path_prefixes` 欠落を全領域アクセス可として扱えること
3. 旧 `UserInfo` の `attributes` 欠落を空集合として解釈できること
4. 既存ユーザを `NoBasicAuth` 未設定ユーザとして継続利用できること

### 9.3 既存運用互換

1. `token create --scope write` が引き続き受理されること
2. `user add` 属性未指定時の従来運用が維持されること
3. `user edit --display-name` / `--password` の既存基本操作が維持されること
4. `token info` / `user info` が追加機能として導入され、既存コマンド呼び出しを置き換えないこと

## 10. 実施レイヤ分割の目安

- 単体テスト向き
  - スコープ文字列と `SCOPE` 欄導出
  - `PATH` / `STATUS` 導出
  - 属性遷移ルール
  - CLI 引数制約
- CLI 結合テスト向き
  - `token create` の成功出力
  - `token list` / `token info` の役割分離
  - `user add` / `user edit` / `user info` の出し分け
- DB連動テスト向き
  - 旧データ読取互換
  - `NoBasicAuth` 遷移後の保存結果
  - path prefix 欠落データの表示導出

## 11. 4.9.4 の整理結果

4.9.4 の完了条件に対する整理結果は以下の通り。

- `token info` の観点を、完全表示項目、非表示項目、全領域アクセス可表示まで含めて定義した
- `user info` の観点を、属性集合、`BASIC AUTH`、token 系との責務分離まで含めて定義した
- 分解スコープ表示の観点を、`SCOPE` 欄の `rcdua`、`write` 互換、保存値と実効権限の分離まで含めて定義した
- path 制約表示の観点を、`PATH` 欄の `*` / `L`、詳細 prefix 群の `token info` への責務分離まで含めて定義した
