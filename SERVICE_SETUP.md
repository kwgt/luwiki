# Windowsサービスのセットアップ

luwikiをWindowsサービスとして登録し、ログオンしていない状態でも起動できるようにする手順です。
サービス実行には`run --win-service`を使用します。
ビルド方法とCLI・設定ファイルの概要は[README](README.md)を参照してください。

## 前提と配置

- Windows用にビルドした`luwiki.exe`を用意します。
- サービスの登録・操作は「管理者として実行」したPowerShellで行います。
- サービスを実行するWindowsアカウントを決め、そのアカウントに「サービスとしてログオン」の権利を付与します。
- 実行アカウントには、実行ファイル・設定ファイルの読み取りと実行、およびデータ保存先への読み書き・作成・削除の権限を付与します。

以下では次の配置を例にします。パスは環境に合わせて変更してください。

```text
C:\Tools\luwiki\luwiki.exe
C:\ProgramData\luwiki\config.toml
C:\ProgramData\luwiki\data\
C:\ProgramData\luwiki\log\
C:\ProgramData\luwiki\audit\
```

サービス名は実装に合わせて`luwiki`、表示名は`LuWiki`とします。
サービスの実行アカウントと、ブラウザで認証するWikiユーザは別のものです。

「サービスとしてログオン」の権利は、ローカルセキュリティポリシーの
「ローカル ポリシー」→「ユーザー権利の割り当て」で設定します。
組織のグループポリシーで管理されている場合は、そのポリシーに従ってください。
詳細はMicrosoftの[サービスログオンアカウントの説明](https://learn.microsoft.com/en-us/windows/win32/ad/about-service-logon-accounts)を参照してください。

## 設定ファイルの準備

サービス実行時の作業ディレクトリやユーザ別のデフォルト保存先に依存しないように、
設定ファイルは絶対パスで指定します。データを配置するディレクトリも事前に作成してください。

```powershell
New-Item -ItemType Directory -Force -Path 'C:\ProgramData\luwiki\data', 'C:\ProgramData\luwiki\log', 'C:\ProgramData\luwiki\audit'
```

`C:\ProgramData\luwiki\config.toml`をUTF-8で作成し、次の内容を記述します。

```toml
[global]
wiki_title = "LuWiki"
db_path = "data/database.redb"
fts_index = "data/index"
assets_path = "data/assets"
log_level = "INFO"
log_output = "log"
asset_limit_size = "10M"
audit_path = "audit"
audit_retention = "90d"
audit_rotate_size = "2M"

[run]
bind = "127.0.0.1"
port = 8080
use_tls = false
use_mcp = false
```

設定ファイル内の相対ファイルパスは、設定ファイルのあるディレクトリを基準に解決されます。
この例では、データベースは`C:\ProgramData\luwiki\data\database.redb`になります。
設定を省略した場合の保存先はサービスの実行アカウントに依存するため、
通常起動から切り替える場合も同じ設定・データを指定してください。

この例の接続先は<http://127.0.0.1:8080/wiki>です。
別のマシンから接続する場合は`bind`を変更し、必要に応じてWindowsファイアウォールを設定してください。
TLSやMCPを使用する場合は、[README](README.md)と[MCPセットアップ](MCP_SETUP.md)を参照してください。
TLS証明書を設定する場合は、実行アカウントに証明書・秘密鍵の読み取り権限も必要です。

## ユーザ登録と通常起動での確認

新規環境では、サービス登録前にWikiユーザを作成します。
既存のデータベースに利用可能なユーザが登録済みの場合は不要です。

```powershell
& 'C:\Tools\luwiki\luwiki.exe' --config-path 'C:\ProgramData\luwiki\config.toml' user add myuser
```

プロンプトに従ってWikiユーザのパスワードを登録します。ユーザ未登録ではサーバを起動できません。
続けて、サービスモードを付けずに通常起動を確認します。

```powershell
& 'C:\Tools\luwiki\luwiki.exe' --config-path 'C:\ProgramData\luwiki\config.toml' run
```

ブラウザで接続し、登録したWikiユーザでBasic認証ができることを確認してください。
確認後は`Ctrl+C`で終了し、サービス起動前に通常起動のプロセスが停止していることを確認します。
可能であればサービスに指定するWindowsアカウントでも確認し、データ保存先へのアクセス権を確かめてください。

`--win-service`はWindowsのサービス制御マネージャー（SCM）経由で起動するときのオプションです。
通常のコンソール起動では付けません。また、サービス起動では`--open-browser`は指定しません。

## サービスの登録

管理者のPowerShellで次を実行します。`DOMAIN\USER`は実行アカウントに置き換えてください。
ローカルアカウントの場合は`コンピュータ名\ユーザ名`を指定します。
パスワードは資格情報の入力画面で入力します。

```powershell
$serviceCredential = Get-Credential -UserName 'DOMAIN\USER' -Message 'LuWikiサービスの実行アカウント'
$serviceCommand = '"C:\Tools\luwiki\luwiki.exe" --config-path "C:\ProgramData\luwiki\config.toml" run --win-service'

New-Service -Name 'luwiki' -DisplayName 'LuWiki' -BinaryPathName $serviceCommand -StartupType Automatic -Credential $serviceCredential
```

実行ファイルと設定ファイルのパスは、空白を含む場合も扱えるように二重引用符で囲んでいます。
`--config-path`は`run`の前に、`--win-service`は`run`の後に指定します。
登録するとWindows起動時の自動起動が有効になります。登録直後は次の手順で起動します。
`New-Service`の詳細は[Microsoftのリファレンス](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.management/new-service)を参照してください。

## 起動と状態確認

```powershell
sc.exe qc luwiki
sc.exe start luwiki
sc.exe query luwiki
```

`qc`で登録された実行コマンド・実行アカウント・起動種別を確認できます。
`query`の状態が`START_PENDING`の場合は、少し待ってから再度確認し、`RUNNING`になったことを確認します。
ブラウザで<http://127.0.0.1:8080/wiki>を開き、ログインとページ表示も確認してください。

PowerShellでは`sc`が別コマンドのエイリアスになる場合があるため、サービス操作には`sc.exe`と明記します。
設定例のアプリケーションログは`C:\ProgramData\luwiki\log`、監査ログは`C:\ProgramData\luwiki\audit`に出力されます。

## 停止・再起動・更新

```powershell
sc.exe stop luwiki
sc.exe query luwiki
```

停止は非同期で進みます。`query`で`STOPPED`になったことを確認してください。
再起動する場合は、停止完了後に`sc.exe start luwiki`を実行します。

実行ファイルの差し替えや設定変更を行う場合も、サービスを停止してから作業し、完了後に起動します。
同じパスで実行ファイルを更新する場合、サービスの再登録は不要です。
既存データの再構成が必要な更新については、[READMEの更新時の注意](README.md#既存データの更新時の注意)を参照してください。
管理CLIを使う場合も、サービスと同じ`--config-path`を指定します。

## サービスの削除

サービスを停止し、`STOPPED`を確認してから削除します。

```powershell
sc.exe delete luwiki
```

サービス登録を削除しても、実行ファイル・設定ファイル・Wikiデータ・ログは残ります。
削除待ちになる場合は「サービス」管理画面などを閉じ、削除が完了してから同名で再登録してください。

## 起動できない場合の確認点

| 症状 | 確認する内容 |
|:--|:--|
| ログオンに失敗する | 実行アカウントとパスワード、「サービスとしてログオン」の権利を確認します。パスワードを変更した場合はサービスのログオン設定も更新します。 |
| ファイルが見つからない／アクセスできない | `sc.exe qc luwiki`で実行ファイル・設定ファイルの絶対パスと引用符を確認し、実行アカウントのアクセス権を確認します。 |
| ユーザ未登録になる／想定したページがない | ユーザ登録時とサービス起動時の設定ファイル・`db_path`が一致しているか確認します。 |
| 待ち受けに失敗する | 通常起動のプロセスや別サービスが同じアドレス・ポートを使用していないか確認します。 |
| サービスがすぐ停止する | アプリケーションログとWindowsイベントビューアーの「Windowsログ」→「システム」を確認します。同じ設定で`--win-service`を外して通常起動し、エラーを確認します。 |

Windowsのサービス登録コマンドについては、Microsoftの[sc.exe create](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/sc-create)も参照できます。
