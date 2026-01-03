# luwiki

ローカル運用を目的とした軽量Wikiシステム

## 使用方法
```text
ローカル運用向けWikiシステム

Usage: luwiki.exe [OPTIONS] [COMMAND]

Commands:
  run       サーバの起動
  user      ユーザ管理コマンド一覧の表示
  page      ページ管理コマンド一覧の表示
  lock      ロック管理コマンド一覧の表示
  asset     アセット管理コマンド一覧の表示
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

  -a, --assets-path <ASSETS_PATH>
          アセットデータ格納ディレクトリのパス

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
  3. ブラウザで 127.0.0.0:8080を開く

## Todo
  - [ ] TSL対応
  - [ ] 全文検索の実装
  - [ ] チェックボックスプラグインの導入
  - [ ] 新規ページ作成インタフェースの追加(URL直打ち or リンク埋め込みでの新規ページ作成は可能)
  - [ ] ページ移動(リネーム)実装
  - [X] ページ削除の実装
  - [X] amend更新の対応
  - [ ] ゾンビページ(削除済みページ)の管理画面
  - [ ] リビジョン履歴管理
  - [ ] エディタ導入(MonacoかCodeMirror)
  - config.toml
      - [ ] Wiki名の表示名設定
      - [ ] アセットアップロードの上限サイズ指定
  - [ ] バックアップ(インポート／エクスポート)機能の実装

## ライセンス
このソフトウェアは[MITライセンス](https://opensource.org/licenses/MIT)の条件下でオープンソースとして利用可能です。
