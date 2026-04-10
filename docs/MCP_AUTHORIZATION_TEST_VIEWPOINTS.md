# MCP認証・認可 テスト観点一覧

本書は、MCPサーバ機能における認証・認可テストの観点を整理した独立したテスト観点書である。

位置付けとしては、`docs/MCP_IMPLEMENTATION_DESIGN_TASKS.md` の 4.9.1 に対応する成果物であり、
MCP 設計書群から参照される実装・テスト実装向け文書として扱う。

MCP では Basic認証を前提とせず Bearer認証を入口とし、
認可では Bearer スコープと path prefix 制約を組み合わせて判定する。
そのため本書では、既存 REST API 向けの Bearer認証観点をそのまま流用せず、
MCP 公開層、path ベースサービス層、MCP 固有の失敗分類を中心に観点を整理する。

## 1. 参照仕様

- `docs/REQUIREMENTS.md`
  - 12. MCPサーバ機能
  - 13. 監査ログ
- `docs/MCP_INTERNAL_DESIGN.md`
  - 5.1 認証・認可
- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - 2.2 path 解決の共通処理
  - 2.3 操作種別ごとの橋渡し
  - 2.5 ユーザ属性モデルの拡張設計
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - 3.6 エラー表現
  - 4. MCP エラー応答と内部エラー分類の対応
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - 3. transport / endpoint 構成
- `docs/BEARER_AUTH_DESIGN.md`
  - 5.2 認証フロー設計
  - 5.3 スコープ判定設計
  - 5.6 エラー処理・テスト方針

## 2. 観点の使い方

- 本書は MCP 実装向けの認証・認可テスト観点を、単体テスト、結合テスト、HTTP 統合テストへ分配するためのたたき台として用いる
- 既存 `docs/BEARER_AUTH_TEST_VIEWPOINTS.md` は REST API を含む共通 Bearer 基盤の観点として維持し、本書では MCP 固有の観点と接続観点を補う
- 1 つの観点で、認証入口、認可判定、エラー応答、監査ログ記録有無を同時に固定するのではなく、何を主確認対象にするかを分けて具体化する
- `NoBasicAuth` は MCP 入口で直接判定する観点ではなく、MCP が Basic を受理しない設計とユーザ属性設計の責務境界を壊していないことを確認する観点として扱う

## 3. MCP認証入口の観点

### 3.1 Bearer資格の受理

1. 正しい Bearer トークン提示時に認証成功し、MCP ハンドラへ認証文脈が渡ること
2. Bearer トークン欠落時に、ツール実行へ進まず認証失敗として扱うこと
3. Bearer トークン形式不正時に、ツール実行へ進まず認証失敗として扱うこと
4. 未発行トークン、照合失敗トークン、失効済みトークン、期限切れトークンの各ケースが認証失敗として区別可能であること
5. トークンに紐付くユーザが解決できない場合に認証失敗として扱うこと
6. 認証失敗時に、MCP の認可判定や業務処理へ進まないこと

### 3.2 MCP入口での方式制約

1. MCP 入口が Basic 認証を受理しないこと
2. Basic 相当の資格情報を持ち込んだ場合でも、Bearer 認証失敗ではなく「MCP では受理しない方式」として扱えること
3. MCP では REST API の `Authorization` 多重指定や Basic / Bearer 併用判定をそのまま持ち込まず、Bearer 前提の薄い入口として成立していること
4. transport レベル入力不備と認証失敗が、後段の認可失敗や業務失敗と混同されないこと

### 3.3 認証文脈の内容

1. 認証成功時の文脈に、操作主体ユーザ、スコープ集合、path prefix 制約集合、`token_id` が含まれること
2. 認証成功時に、操作対象 path を見た認可判定が入口で先走って行われないこと
3. 認証文脈が後続 service 層へ渡され、操作種別ごとの required scope 判定に利用できること
4. 認証成功時に必要な TTL 延長が Bearer 認証コア側で反映され、MCP 入口が独自実装を持たないこと

## 4. スコープ判定の観点

### 4.1 read系操作

1. `get_page`、`list_pages`、`search_pages`、`get_page_section` が `read` で許可されること
2. `write` を持つトークンで read 系操作が許可されること
3. `create`、`update`、`append`、`delete` のみを持ち `read` を持たないトークンでは、read 系操作が許可されないこと
4. スコープ不足時に認可失敗となり、認証失敗と混同されないこと

### 4.2 write系操作

1. `create_page` が `create` で許可されること
2. `update_page` が `update` で許可されること
3. `edit_page` が `update` で許可されること
4. `append_page` が `append` で許可されること
5. `rename_page` が `update` で許可されること
6. `write` を持つトークンで `create_page`、`update_page`、`edit_page`、`append_page`、`rename_page` が許可されること
7. `read` のみを持つトークンで write 系操作が拒否されること
8. `append` のみを持つトークンで `update_page` と `edit_page` が拒否されること
9. `update` のみを持つトークンで `append_page` が拒否されること
10. 分解済みスコープのみを複数持っていても `write` 要求そのものを満たしたと誤判定しないこと

### 4.3 スコープ集合の境界

1. 重複を含むスコープ指定でも集合として正しく評価されること
2. 保存値として `write` を保持した場合に、判定時のみ上位互換として機能すること
3. 保存値として `read` / `create` / `update` / `append` / `delete` を保持した場合に、暗黙包含を行わないこと
4. 必要スコープがツールごとに固定され、汎用ツール内の分岐でぶれないこと

## 5. path prefix 制約の観点

### 5.1 単一path操作

1. `get_page` が current path 基準で path prefix 制約判定されること
2. `update_page` が current path 基準で path prefix 制約判定されること
3. `edit_page` が current path 基準で path prefix 制約判定されること
4. `append_page` が current path 基準で path prefix 制約判定されること
5. `create_page` が target path 基準で path prefix 制約判定されること
6. `rename_page` が移動元 path と移動先 path の双方で path prefix 制約判定されること
7. `rename_page` で片側のみ許可範囲内のとき拒否されること
8. rename 後の旧 path 指定が自動追跡されず `not found` 系として扱われること

### 5.2 list / search の prefix 指定

1. `list_pages` で prefix 指定がある場合、要求 prefix 自体が認可判定対象になること
2. `search_pages` で prefix 指定がある場合、要求 prefix 自体が認可判定対象になること
3. prefix 指定が許可範囲外の場合、結果後段フィルタ以前に拒否されること
4. prefix 未指定時は要求 prefix 判定を行わず、結果後段フィルタだけで許可範囲外を除外すること
5. 許可範囲内 prefix 指定でも、検索結果や一覧結果に混在した許可範囲外 path が返却されないこと

### 5.3 正規化と境界一致

1. path prefix 制約は正規化済み絶対パス同士で比較されること
2. `/docs` が `/docs/a` を許可し `/docs2` を誤って許可しないこと
3. root `/` を保持したトークンが全領域アクセス可として扱われること
4. 複数 prefix 指定時に、いずれか 1 つに一致すれば許可されること
5. path 入力不正と path prefix 制約違反が別失敗分類として扱われること

## 6. NoBasicAuth の観点

### 6.1 責務境界

1. `NoBasicAuth` は Basic 認証拒否のためのユーザ属性であり、MCP 認可属性として扱われないこと
2. MCP 入口が Basic を受理しないため、MCP 認証・認可の可否が `NoBasicAuth` の有無で変化しないこと
3. Bearer 認証成功後の MCP 認可判定で `NoBasicAuth` を参照しないこと

### 6.2 回帰確認

1. `NoBasicAuth` ユーザに発行した Bearer トークンで MCP 操作できること
2. 同一ユーザが REST API の Basic 認証では拒否されること
3. `NoBasicAuth` の有無によって Bearer スコープ判定や path prefix 制約判定が変化しないこと

## 7. エラー応答と監査接続の観点

### 7.1 認証失敗と認可失敗の分離

1. 認証失敗が MCP ツール実行エラーへ混在しないこと
2. スコープ不足が認証失敗ではなく認可失敗として返ること
3. path prefix 制約違反が認証失敗ではなく認可失敗として返ること
4. path 不正、対象ページ不存在、競合が認証失敗と誤分類されないこと

### 7.2 監査ログ接続

1. 認証失敗が監査ログではなく HTTP ログ側で扱われること
2. スコープ不足が監査ログ対象の認可失敗として記録されること
3. path prefix 制約違反が監査ログ対象の認可失敗として記録されること
4. 認可失敗時でも Bearer 認証成功後であれば `token_id` を監査ログへ残せること

## 8. 実施レイヤ分割の目安

- 単体テスト向き
  - スコープ集合の包含判定
  - path prefix 境界一致判定
  - required scope とツール種別の対応
- サービス層結合テスト向き
  - current path / target path / rename 両側 path の認可判定
  - list / search の要求 prefix 判定と後段フィルタの組み合わせ
  - `write` 互換と分解済みスコープ非包含の確認
- transport / HTTP 統合テスト向き
  - Bearer 資格の抽出
  - Basic 不受理
  - 認証失敗と認可失敗の責務分離
  - 監査ログ記録有無の確認

## 9. 4.9.1 の整理結果

4.9.1 の完了条件に対する整理結果は以下の通り。

- Bearer 認証成功の観点を、MCP 入口、認証文脈、TTL 延長連携まで含めて定義した
- スコープ不足の観点を、read 系、write 系、`write` 互換、分解済みスコープ境界まで含めて定義した
- path prefix 制約違反の観点を、単一 path 操作、list / search の要求 prefix、結果後段フィルタ、rename 両側判定まで含めて定義した
- `NoBasicAuth` の観点を、MCP で直接判定しない責務境界と、Bearer 利用時の回帰確認に分けて定義した
