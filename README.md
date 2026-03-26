# luwiki

ローカル運用を目的とした軽量Wikiシステム

## 使用方法
```text
ローカル運用向けWikiシステム

Usage: luwiki [OPTIONS] [COMMAND]

Commands:
  run       サーバの起動
  user      ユーザ管理コマンド一覧の表示
  page      ページ管理コマンド一覧の表示
  lock      ロック管理コマンド一覧の表示
  asset     アセット管理コマンド一覧の表示
  fts       全文検索管理コマンド一覧の表示
  token     Bearerトークン管理コマンド一覧の表示
  export    バックアップ／マイグレート用データのエクスポート
  import    エクスポートデータのインポート
  commands  サブコマンド一覧の表示
  help-all  全サブコマンドのヘルプ出力
  help      Print this message or the help of the given subcommand(s)

Options:
  -c, --config-path <CONFIG_PATH>
          config.tomlを使用する場合のパス

  -l, --log-level <LEVEL>
          記録するログレベルの指定

          Possible values:
          - NONE:  ログを記録しない
          - ERROR: エラー情報以上のレベルを記録
          - WARN:  警告情報以上のレベルを記録
          - INFO:  一般情報以上のレベルを記録
          - DEBUG: デバッグ情報以上のレベルを記録
          - TRACE: トレース情報以上のレベルを記録

  -L, --log-output <PATH>
          ログの出力先の指定

      --log-tee
          ログを標準出力にも同時出力するか否か

  -d, --db-path <DB_PATH>
          データベースファイルのパス

  -I, --fts-index <FTS_INDEX>
          全文検索インデックスの格納パス

  -a, --assets-path <ASSETS_PATH>
          アセットデータ格納ディレクトリのパス

  -t, --template-root <PATH>
          テンプレートページの格納パス

  -T, --wiki-title <TITLE>
          Wikiタイトル

  -S, --asset-limit-size <SIZE>
          アセットサイズ上限

      --audit-log-dir <DIR>
          監査ログ出力先

      --audit-log-retention <DURATION>
          監査ログ保持期間

      --audit-log-rotate-size <SIZE>
          監査ログローテーション閾値

      --show-options
          設定情報の表示

      --save-config
          設定情報の保存

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

## ブートストラップ

  1. ユーザの追加 : `user add`サブコマンドでユーザを追加
  2. サーバの起動 : `run`サブコマンドでサーバを起動
  3. ブラウザで http://127.0.0.1:8080/wiki を開く

## MCPサーバ機能を使用する場合
[こちら](MCP_SETUP.md)をご覧ください。

## Todo
  - [x] TLS対応
  - [x] 全文検索の実装
  - [x] 新規ページ作成インタフェースの追加(URL直打ち or リンク埋め込みでの新規ページ作成は可能)
  - [x] ページ移動(リネーム)実装
  - [x] ページ削除の実装
  - [x] amend更新の対応
  - [x] ゾンビページ(削除済みページ)の管理画面
  - [x] リビジョン管理画面
  - [x] エディタ導入(CodeMirror)
  - config.toml
      - [X] Wiki名の表示名設定
      - [X] アセットアップロードの上限サイズ指定
      - [ ] アイコン設定
  - [x] バックアップ(インポート／エクスポート)機能の実装
  - Markdown追加機能
      - [x] Github風チェックボックス
      - [x] Mermaid対応 (Mermaid.js組み込み)
      - [x] 数式対応 (@mdit/plugin-katexを使用)
  - [x] テンプレート機能
  - マクロ機能
      - [x] コメント記述マクロ
      - [x] 子ページリスト生成マクロ
      - [x] 単純リンク生成
      - [x] アセットのコードブロック展開マクロ
      - [x] ページ名展開
  - [x] Bearer認証のサポート
  - MCPサーバ機能
      - tools
          - ページ本文の操作
              - [x] get_page
              - [x] get_page_toc
              - [x] list_pages
              - [x] search_pages
              - [x] create_page
              - [x] update_page
              - [x] append_page
              - [x] rename_page
              - [x] get_page_section
              - [ ] edit_page
          - [ ] アセットの操作
      - resources
          - [ ] テンプレート
      - [ ] prompts

## ライセンス
このソフトウェアは[MITライセンス](https://opensource.org/licenses/MIT)の条件下でオープンソースとして利用可能です。
