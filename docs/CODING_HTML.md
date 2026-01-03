# HTMLにおけるコーディング規約

本ドキュメントは、本プロジェクトにおけるHTML テンプレートの設計および記述規約を定めるものである。

本プロジェクトでは以下を原則とする。

- HTML はビルド成果物（生成物）として扱う
- 人が直接記述・レビューするテンプレートはPugを用いる
- 本規約はPug によるテンプレート記述規約を中心に定義する

---

## 2. 基本方針

### 2.1 記述対象

- 手書き対象: `*.pug`
- 生成物: `*.html`（リポジトリ管理対象外）

### 2.2 設計思想

- 可読性を最優先する
- DOM 構造が Pug のインデントから直感的に把握できること
- JavaScript / CSS との責務分離を徹底する
- 「見た目」ではなく「意味（セマンティクス）」で構造を定義する

---

## 3. Pug 記述ルール

### 3.1 インデント

- タブ文字は使用せず空白文字2個を単位とする
- インデント = DOM 階層 と一致させる

```pug
main
  section
    h1 Title
    p Description
```

### 3.2 タグの省略と明示

- div は原則省略可
- セマンティック要素は明示的に使用する

```pug
// good
section.content
  p Text

// bad
div.content
  p Text
```

### 3.3 クラス・ID の記法

- クラスは名はkebab-caseで木ジュルすること
- ID は原則使用しない（JS 連携が必要な場合のみ）

```pug
article.user-card
button.submit-button
```

### 3.4 属性の記述

- 属性が多い場合は 改行して記述
- 属性は論理的なまとまり順に並べる

```pug
input(
  type="text"
  name="username"
  placeholder="User name"
  autocomplete="username"
)
```

