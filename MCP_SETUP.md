# MCPサーバ機能のセットアップ例

MCPサーバ機能を有効にすると、AIエージェント（Codex CLI / Claude Code など）からWikiのページ操作（取得・更新・検索など）を行うことができます。

本書ではMCPサーバ機能を有効にする場合の設定例を記述します。

## 手順
以下の手順で準備。

  1. エージェント用アカウントの作成
  2. アクセストークンの発行
  3. プライベートCAとサーバ証明書の準備、TLSを有効にしたサーバの起動
  4. クライアントでのCA証明書の信頼設定
  5. エージェントの設定追加・起動

以下に個々の手順を記述します。

### エージェント用アカウントの発行
`user add`コマンドでAIエージェント用のアカウントを作成します。このとき、属性に"no_basic_auth"を付与し、ブラウザからログインできないようにしておいてください。

```sh
luwiki user add --attribute no_basic_auth -d "AI-Agent" agent
```

### アクセストークンの発行
`token create`コマンドでAIエージェント用のアクセストークンを生成します。以下の様にアクセストークンの生成を行います。

```sh
luwiki token create --scope read,create,update --path-prefix "/JailPage" agent
```

オプションの意味は以下のとおりです。

- `--scope`: 許可する権限を指定。read, create, delete, update, appendが指定可能。writeはすべてを指定する場合のエイリアス。指定しなかった場合はwriteを指定した場合と同じ。可能な限り最小限の権限で運用してください。
- `--path-prefix` : アクセス可能なページのパスプレフィクスを指定。複数指定可。指定しなかった場合は全ページにアクセス可能。
以下の例では、/JailPage以下のページに対し読み書き自由な権限を持つアクセストークンを生成します。

上記を実行しアクセストークンが生成されると以下のような表示が行われます。

```text
TOKEN ID:     01KN1649ABNQ9HCSNYW01F8RNT
TOKEN NAME:   -
USERNAME:     agent
SCOPES:       read, create, delete, update
PERMISSIONS:  read, create, delete, update
TTL:          30d
PATH PREFIXES:
    - /JailPage
TIMESTAMPS:
    create: 2026-03-31T14:33:41
    expire: 2026-04-30T14:33:41

TOKEN VALUE:
    9AVqOTCQ/h4igewTZE5HhZ5K0eFwGRPCwkr0fXBInro=
```

上記の"TOKEN VALUE"の欄に表示された文字列がアクセストークンになります(上記の例であれば`9AVqOTCQ/h4igewTZE5HhZ5K0eFwGRPCwkr0fXBInro=`)。このアクセストークンは生成時にしか表示されないので注意してください。

### プライベートCAとサーバ証明書の準備

MCPクライアントとの互換性を確保するため、本書では自己署名のルートCAを作り、
そのCAで専用のサーバ証明書を署名する構成を推奨します。

> Codex CLIの公式資料はカスタムCAバンドル、Claude Codeの公式資料はカスタムCA証明書の指定を案内しています。自己署名サーバ証明書そのものの直接登録は、TLS実装や証明書の拡張属性によって扱いが異なり、すべてのエージェントで使える手順とはしません。

この方法では以下のファイルを作成する必要があります。

| ファイル | 用途・配置先 |
|:--|:--|
| `root-ca.pem` | ルートCA証明書。クライアントへ配布し、信頼対象に指定します。 |
| `root-ca-key.pem` | CA秘密鍵。発行担当者が保管し、クライアントや稼働サーバには配布しません。 |
| `server-cert.pem` | CAが発行したサーバ証明書。luwikiの`--cert`へ指定します。 |
| `server-key.pem` | サーバ秘密鍵。luwikiの`--key`へ指定し、クライアントには配布しません。 |

#### OpenSSLによる発行例

以下はOpenSSL 3.xを使い、ルートCAから直接サーバ証明書を発行する小規模なローカル運用の例です。
既存の組織内CAがある場合は、そのCAで接続先名に対応するサーバ証明書を発行してください。
コマンドは証明書作業用の新しいディレクトリで実行し、既存の鍵・証明書を上書きしないでください。
WindowsではOpenSSLを利用できるPowerShellで実行します。

1. ルートCAの鍵と自己署名CA証明書を作成します。

   ```powershell
   openssl req -x509 -newkey rsa:3072 -sha256 -days 3650 -keyout root-ca-key.pem -out root-ca.pem -subj "/CN=LuWiki Local Root CA" -addext "basicConstraints=critical,CA:TRUE,pathlen:0" -addext "keyUsage=critical,keyCertSign,cRLSign" -addext "subjectKeyIdentifier=hash"
   ```

   CA秘密鍵を暗号化するためのパスフレーズを対話入力します。CA秘密鍵とパスフレーズは保管してください。
   `pathlen:0`は、この例では中間CAを作らず、サーバ証明書を直接発行することを意味します。

2. サーバ秘密鍵と証明書署名要求（CSR）を作成します。

   ```powershell
   openssl req -new -newkey rsa:3072 -noenc -sha256 -keyout server-key.pem -out server.csr -subj "/CN=localhost"
   ```

   サーバ秘密鍵はサービスの無人起動用に暗号化しないため、OSのアクセス権で読み取りを制限してください。

3. `server-ext.cnf`を次の内容で作成します。

   ```ini
   [server_cert]
   basicConstraints = critical,CA:FALSE
   keyUsage = critical,digitalSignature,keyEncipherment
   extendedKeyUsage = serverAuth
   subjectKeyIdentifier = hash
   authorityKeyIdentifier = keyid,issuer
   subjectAltName = @alt_names

   [alt_names]
   DNS.1 = localhost
   IP.1 = 127.0.0.1
   ```

   接続URLのホスト名・IPアドレスをSAN（`alt_names`）に列挙します。
   別マシンから接続する場合は、例えば`DNS.2 = wiki.example.internal`や`IP.2 = 192.168.1.10`を追加します。
   Common Name（CN）だけの指定では不十分です。

4. CAでサーバ証明書を署名し、チェーンと用途・接続先名を確認します。

   ```powershell
   openssl x509 -req -in server.csr -CA root-ca.pem -CAkey root-ca-key.pem -CAcreateserial -out server-cert.pem -days 365 -sha256 -extfile server-ext.cnf -extensions server_cert
   openssl verify -CAfile root-ca.pem -purpose sslserver -verify_hostname localhost server-cert.pem
   openssl verify -CAfile root-ca.pem -purpose sslserver -verify_ip 127.0.0.1 server-cert.pem
   ```

   署名時はCA秘密鍵のパスフレーズを入力します。検証結果が`server-cert.pem: OK`であることを確認します。
   別の接続先名を使用する場合も、その名前で検証してください。

コマンドと拡張属性の詳細はOpenSSLの[req](https://docs.openssl.org/3.0/man1/openssl-req/)、
[x509](https://docs.openssl.org/3.0/man1/openssl-x509/)、
[X.509拡張設定](https://docs.openssl.org/3.0/man5/x509v3_config/)を参照してください。

#### サーバ証明書を明示して起動

`server-cert.pem`と`server-key.pem`をサーバの配置先へコピーし、実際の絶対パスを指定して起動します。
以下は同一マシンからHTTPSで接続する例です。`--mcp`でMCP機能を、`--tls`でTLSを有効にします。
別マシンから接続する場合は、SANに加えて待ち受け先のアドレスとファイアウォールも接続先に合わせて設定してください。

```powershell
luwiki run --mcp --tls --cert C:/ProgramData/luwiki/tls/server-cert.pem --key C:/ProgramData/luwiki/tls/server-key.pem 127.0.0.1:8080
```

設定ファイルで指定する場合は、既存の`[run]`へ次の項目を追加・更新します。

```toml
[run]
bind = "127.0.0.1"
port = 8080
use_tls = true
use_mcp = true
server_cert = "tls/server-cert.pem"
server_key = "tls/server-key.pem"
```

相対ファイルパスは設定ファイルのあるディレクトリ基準です。
通常起動時に`--config-path`などを使用している場合は、ユーザ登録・トークン発行・サーバ起動にも同じ共通オプションを指定してください。

この例はルートCAが直接発行するため、`--cert`のファイルにはサーバ証明書だけを入れます。
中間CAを使用する構成では、サーバ証明書、中間CA証明書の順にPEMを連結したチェーンを`--cert`へ指定します。
ルートCA証明書は通常チェーンに含めず、クライアント側へ配布して明示的に信頼させます。
サーバからCA証明書を送るだけでは、クライアント側の信頼設定の代わりにはなりません。

#### 自動生成の自己署名サーバ証明書について

`luwiki run --mcp --tls`は、既定の証明書ファイルがない場合、サーバ証明書と秘密鍵を含む`server.pem`を自動生成しますが、これはプライベートCAの開設やCAとサーバ証明書の分離を行う機能ではありません。
このファイルから証明書を取り出して信頼対象にするだけで、すべてのMCPクライアントが接続できるとは限りません。
エージェントとの接続を行う場合は、前述のCA発行証明書を明示する構成へ切り替えてください。

### クライアントでのCA証明書の信頼設定

発行担当者が確認した`root-ca.pem`だけをクライアントへ配布します。
以下のWindows例では`%USERPROFILE%\luwiki-root-ca.pem`に保存したものとします。
クライアントが信頼するのはサーバ証明書や秘密鍵ではなく、このルートCA証明書です。
次の設定は、Wikiサーバへ接続する**Codex CLI／Claude Codeを起動する側**で行います。
WikiサーバやWindowsサービスに環境変数を設定しても、別プロセスのCLIには反映されません。
ブラウザで証明書の警告を許可しただけでも、CLIの信頼設定は完了しません。

同じCAでサーバ証明書を更新した場合は、通常クライアント側のCA証明書を変更する必要はありません。
CAを変更・更新する場合は、クライアント側の信頼設定も更新します。

#### Codex CLIの起動

ルートCAのPEMファイルの絶対パスを`CODEX_CA_CERTIFICATE`に設定し、同じシェルからCodex CLIを起動します。

```powershell
$env:CODEX_CA_CERTIFICATE = Join-Path $env:USERPROFILE 'luwiki-root-ca.pem'
codex
```

Linuxなどのシェルでは次のように指定します。

```sh
CODEX_CA_CERTIFICATE=/absolute/path/to/luwiki-root-ca.pem codex
```

`CODEX_CA_CERTIFICATE`が未設定の場合は`SSL_CERT_FILE`が使われます。
既に`CODEX_CA_CERTIFICATE`が設定されている環境では、`SSL_CERT_FILE`だけを変更しても切り替わりません。
詳細はOpenAIの[Custom CA bundles](https://learn.chatgpt.com/docs/auth#custom-ca-bundles)を参照してください。

#### Claude Codeの起動

ルートCAのPEMファイルの絶対パスを`NODE_EXTRA_CA_CERTS`に設定し、同じシェルからClaude Codeを起動します。

```powershell
$env:NODE_EXTRA_CA_CERTS = Join-Path $env:USERPROFILE 'luwiki-root-ca.pem'
claude
```

```sh
NODE_EXTRA_CA_CERTS=/absolute/path/to/luwiki-root-ca.pem claude
```

これらの起動例はローカルのCLI向けです。起動済みのCLIは終了し、環境変数を設定したシェルから起動し直してください。
PowerShellの`$env:`設定は現在のセッションと、その後に起動する子プロセスに適用されます。
詳細はClaude Codeの[Custom CA certificates](https://code.claude.com/docs/en/network-config#custom-ca-certificates)を参照してください。

#### 接続先の名前とエラーの確認

証明書を信頼させる設定と、接続先の名前が証明書に一致することは、両方とも必要です。
本書の発行例ではサーバ証明書のSANに`localhost`と`127.0.0.1`を含めています。
クライアントのURLには`https://localhost:8080/mcp`または`https://127.0.0.1:8080/mcp`を指定してください。
別マシンのホスト名・LANアドレス・IPv6の`::1`で接続する場合は、
その接続先をSANに含む証明書を別途用意し、サーバの`--cert`・`--key`で指定します。

WindowsでTLSの接続確認を行う例です。Bearerトークンを送らず、証明書検証を含むHTTP接続だけを確認します。

```powershell
curl.exe --cacert "$env:USERPROFILE\luwiki-root-ca.pem" --output NUL --write-out "%{http_code}\n" https://localhost:8080/wiki
```

HTTPの`401`が返れば、TLS接続は成功し、その先のBasic認証が必要な状態です。
この確認だけでMCPの認証・接続まで成功したことにはなりません。

| エラー・症状 | 確認する内容 |
|:--|:--|
| 自己署名証明書・不明な発行者のエラー | CLIが発行元のルートCAを信頼しているか、サーバが指定した証明書を提示しているか、中間CAが必要な場合はチェーンが揃っているか確認します。 |
| CAをサーバ証明書に使ったエラー／用途のエラー | `--cert`にCA証明書を指定していないか確認します。CAは`CA:TRUE`、サーバは`CA:FALSE`と`serverAuth`を持つ別の証明書にします。 |
| ホスト名・IPアドレスの不一致 | URLのホスト名またはIPアドレスが証明書のSANに含まれているか確認します。信頼設定の追加だけでは解決しません。 |
| 有効期限のエラー | 証明書の有効期間とクライアント・サーバの時刻を確認します。 |
| TLS接続後の`401`・`403` | MCPのBearerトークン、有効期限、スコープ、path prefixを確認します。証明書の信頼設定とは別の問題です。 |
| TLSの通信形式に関するエラー | サーバで`--tls`を指定しているか、接続URLの`http://`／`https://`が一致しているか確認します。 |

証明書検証を無効化する設定は使わず、信頼対象の証明書と接続先名を合わせてください。

### エージェントの設定追加

以下の例の`${サーバのアドレス}`と`${アクセストークン}`は、実際の値に置き換えてください。
本書の発行例を使う同一マシン構成では、サーバのアドレスは`localhost`または`127.0.0.1`です。
プライベートCAを利用する場合は、前述の環境変数を設定したシェルから各CLIを起動します。

#### Codex CLIの場合
`~/.codex/config.toml`を編集し以下のようなエントリを追加します。

```toml
[mcp_servers.local_wiki]
url = "https://${サーバのアドレス}:8080/mcp"
http_headers = { "Authorization" = "Bearer ${アクセストークン}" }
```

#### Claude Codeの場合
##### claudeコマンドを使用する場合
以下の様に`mcp add`サブコマンドを用いて登録を行います。

```sh
claude mcp add --transport http local_wiki  "https://${サーバのアドレス}:8080/mcp" --header "Authorization: Bearer ${アクセストークン}"
```

##### .claude.jsonを編集する場合
`~/.claude.json`を編集し、"mcpServers"に以下のようなエントリを追加します。

```json
"mcpServers": {
  "local_wiki": {
    "type": "http",
    "url": "https://${サーバのアドレス}:8080/mcp",
    "headers": {
      "Authorization": "Bearer ${アクセストークン}"
    }
  }
}
```
