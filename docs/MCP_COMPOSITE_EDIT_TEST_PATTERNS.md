# OP3-MS2 複合編集テストパターン実施一覧

作成日 2026-11-13

## 対象と実行結果

実装済みテストの入力・assertから、実際に実行した複合編集パターンを列挙した。編集・追跡層の11テスト関数を分岐・ループ単位へ展開した25パターンに、サービス層2パターン、監査1パターン、HTTP/MCP境界2パターンを加え、計30パターンを掲載する。追跡層の残る1関数はbyte境界と桁溢れの内部防御テストであり、補助検証に記載する。

根拠となる実行結果は以下。

| 実行 | 結果 |
| --- | --- |
| cargo test --lib mcp:: -- --test-threads=4 | 275 passed / 0 failed、40.91秒 |
| cargo test --lib mcp::service::sections::batch -- --test-threads=4 | 12 passed / 0 failed |
| cargo test --lib skill_setup_batch_example_uses_original_sections -- --test-threads=1 | 1 passed / 0 failed |
| cargo check | 成功 |

上記はMS2実装時の実行記録。本書の出力作業では再実行していない。各パターンの「結果」は、そのパターンを含むテスト関数内のassertが成功したことを示す。途中本文・内部状態の逐次ダンプは収集していない。

## 読み方

- 初期IDは初期本文の見出し順でs-001、s-002…となる。
- operation欄は、Rustテストで組み立てた操作をJSON形式へ展開したもの。公開HTTPで直接送信する試験は、その旨を照合内容へ記載する。
- 省略されたtitle・include_subsection・occurrenceは、元テストでも未指定。include_subsectionはfalse、occurrenceはfirstに相当する。
- エラーの期待結果と、テストが実際にassertした範囲を記載する。本文完全一致、部分一致、code、message等の確認範囲を区別する。
- 大本文の1ケースは、初期本文を生成するRust式を記載する。

## パターン索引

| 番号 | パターン | 操作数 | 結果 |
| --- | --- | --- | --- |
| 1 | [前方へ同名見出しを挿入してから初期IDを編集](#pattern-1) | 2 | 成功 |
| 2 | [前方へ同名見出しを挿入してから初期titleを編集](#pattern-2) | 2 | 成功 |
| 3 | [見出し名変更後に初期titleで元節を再編集](#pattern-3) | 2 | 成功 |
| 4 | [見出し名変更後の新titleを初期selectorとして指定](#pattern-4) | 2 | 成功 |
| 5 | [後続selectorが曖昧でも先行操作の不一致を先に返す](#pattern-5) | 2 | 成功 |
| 6 | [片方の同名節を削除しても初期titleの曖昧性を保持](#pattern-6) | 2 | 成功 |
| 7 | [親本文の置換で子を保持し、その子を編集](#pattern-7) | 2 | 成功 |
| 8 | [子節込み親置換後に元の子を参照](#pattern-8) | 2 | 成功 |
| 9 | [親の置換差分が同一本文なら子の追跡を保持](#pattern-9) | 2 | 成功 |
| 10 | [子の前へ新しい子を挿入してから元の子だけ削除](#pattern-10) | 2 | 成功 |
| 11 | [子の前へ新しい子を挿入してから親を削除](#pattern-11) | 2 | 成功 |
| 12 | [同レベル節をafterへ挿入してから元anchorを削除](#pattern-12) | 2 | 成功 |
| 13 | [親削除後に元の子を編集](#pattern-13) | 2 | 成功 |
| 14 | [新設節の本文をreplace_text allで編集](#pattern-14) | 2 | 成功 |
| 15 | [新設titleを同じ要求のsection selectorで参照](#pattern-15) | 2 | 成功 |
| 16 | [replace_textで見出し行を変更した後の元節参照](#pattern-16) | 2 | 成功 |
| 17 | [コードフェンスで見出しを隠した後の元節参照](#pattern-17) | 2 | 成功 |
| 18 | [見出しの同一文字列置換後の元節参照](#pattern-18) | 2 | 成功 |
| 19 | [フェンスを除去して本文を戻しても追跡不能は復活しない](#pattern-19) | 3 | 成功 |
| 20 | [削除後に同じ位置へ同名節を再作成しても元IDへ結合しない](#pattern-20) | 3 | 成功 |
| 21 | [同じanchorへのbefore連続挿入](#pattern-21) | 2 | 成功 |
| 22 | [同じanchorへのafter連続挿入](#pattern-22) | 2 | 成功 |
| 23 | [日本語Setext見出しの変更と複数節編集、末尾改行なし](#pattern-23) | 3 | 成功 |
| 24 | [装飾・codeを含む見出しを初期titleで再編集](#pattern-24) | 2 | 成功 |
| 25 | [大きい日本語本文への32回の同一置換と元節編集](#pattern-25) | 33 | 成功 |
| 26 | [二回の置換で元本文へ戻した要求を一回保存](#pattern-26) | 2 | 成功 |
| 27 | [先行置換成功後に後続置換が失敗した要求の未保存](#pattern-27) | 2 | 成功 |
| 28 | [二操作の一要求でUpdate監査を一件記録](#pattern-28) | 2 | 成功 |
| 29 | [HTTP tools/callで二段階置換と一revision保存](#pattern-29) | 2 | 成功 |
| 30 | [SKILL_SETUP.md掲載JSON例をHTTP tools/callで適用](#pattern-30) | 2 | 成功 |

## 実行した複合パターン

<a id="pattern-1"></a>

### 1. 前方へ同名見出しを挿入してから初期IDを編集

- テスト関数 `initial_identity_survives_insertion_and_duplicate_title`
- コード位置 [src/mcp/service/sections/batch/tests.rs:96](../src/mcp/service/sections/batch/tests.rs#L96)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# A
a
# B
b
~~~

操作列

~~~json
[
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-001"
    },
    "placement": "before",
    "content": "# B\nnew\n"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-002"
    },
    "content": "changed"
  }
]
~~~

期待結果

~~~text
# B
new
# A
a
# B
changed
~~~

<a id="pattern-2"></a>

### 2. 前方へ同名見出しを挿入してから初期titleを編集

- テスト関数 `initial_identity_survives_insertion_and_duplicate_title`
- コード位置 [src/mcp/service/sections/batch/tests.rs:96](../src/mcp/service/sections/batch/tests.rs#L96)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# A
a
# B
b
~~~

操作列

~~~json
[
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-001"
    },
    "placement": "before",
    "content": "# B\nnew\n"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "title",
      "value": "B"
    },
    "content": "changed"
  }
]
~~~

期待結果

~~~text
# B
new
# A
a
# B
changed
~~~

<a id="pattern-3"></a>

### 3. 見出し名変更後に初期titleで元節を再編集

- テスト関数 `explicit_title_change_preserves_original_binding`
- コード位置 [src/mcp/service/sections/batch/tests.rs:125](../src/mcp/service/sections/batch/tests.rs#L125)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# A
a
~~~

操作列

~~~json
[
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "one",
    "title": "Z"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "title",
      "value": "A"
    },
    "content": "two"
  }
]
~~~

期待結果

~~~text
# Z
two
~~~

<a id="pattern-4"></a>

### 4. 見出し名変更後の新titleを初期selectorとして指定

- テスト関数 `explicit_title_change_preserves_original_binding`
- コード位置 [src/mcp/service/sections/batch/tests.rs:125](../src/mcp/service/sections/batch/tests.rs#L125)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 エラーcodeとmessageの operation 2 failed: 接頭辞

初期本文

~~~text
# A
a
~~~

操作列

~~~json
[
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "one",
    "title": "Z"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "title",
      "value": "Z"
    },
    "content": "two"
  }
]
~~~

期待結果

~~~text
not_found
~~~

<a id="pattern-5"></a>

### 5. 後続selectorが曖昧でも先行操作の不一致を先に返す

- テスト関数 `binding_errors_are_delayed_until_their_operation`
- コード位置 [src/mcp/service/sections/batch/tests.rs:158](../src/mcp/service/sections/batch/tests.rs#L158)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 エラーcodeとmessageの operation 1 failed: 接頭辞

初期本文

~~~text
# B
a
# B
b
~~~

操作列

~~~json
[
  {
    "type": "replace_text",
    "old_text": "missing",
    "new_text": "x"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "title",
      "value": "B"
    },
    "content": "new"
  }
]
~~~

期待結果

~~~text
not_found
~~~

<a id="pattern-6"></a>

### 6. 片方の同名節を削除しても初期titleの曖昧性を保持

- テスト関数 `binding_errors_are_delayed_until_their_operation`
- コード位置 [src/mcp/service/sections/batch/tests.rs:158](../src/mcp/service/sections/batch/tests.rs#L158)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 エラーcode

初期本文

~~~text
# B
a
# B
b
~~~

操作列

~~~json
[
  {
    "type": "delete_section",
    "section": {
      "by": "id",
      "value": "s-001"
    }
  },
  {
    "type": "replace_section",
    "section": {
      "by": "title",
      "value": "B"
    },
    "content": "new"
  }
]
~~~

期待結果

~~~text
invalid_input
~~~

<a id="pattern-7"></a>

### 7. 親本文の置換で子を保持し、その子を編集

- テスト関数 `parent_replacement_preserves_or_deletes_original_children`
- コード位置 [src/mcp/service/sections/batch/tests.rs:188](../src/mcp/service/sections/batch/tests.rs#L188)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# P
p
## C
c
# B
b
~~~

操作列

~~~json
[
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "parent",
    "include_subsection": false
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-002"
    },
    "content": "child"
  }
]
~~~

期待結果

~~~text
# P
parent
## C
child
# B
b
~~~

<a id="pattern-8"></a>

### 8. 子節込み親置換後に元の子を参照

- テスト関数 `parent_replacement_preserves_or_deletes_original_children`
- コード位置 [src/mcp/service/sections/batch/tests.rs:188](../src/mcp/service/sections/batch/tests.rs#L188)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 エラーcode

初期本文

~~~text
# P
p
## C
c
# B
b
~~~

操作列

~~~json
[
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "parent",
    "include_subsection": true
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-002"
    },
    "content": "child"
  }
]
~~~

期待結果

~~~text
not_found
~~~

<a id="pattern-9"></a>

### 9. 親の置換差分が同一本文なら子の追跡を保持

- テスト関数 `parent_replacement_preserves_or_deletes_original_children`
- コード位置 [src/mcp/service/sections/batch/tests.rs:188](../src/mcp/service/sections/batch/tests.rs#L188)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# P
## C
c
~~~

操作列

~~~json
[
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "## C\nc",
    "include_subsection": true
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-002"
    },
    "content": "kept"
  }
]
~~~

期待結果

~~~text
# P
## C
kept
~~~

<a id="pattern-10"></a>

### 10. 子の前へ新しい子を挿入してから元の子だけ削除

- テスト関数 `deletion_uses_current_parent_and_sibling_ranges`
- コード位置 [src/mcp/service/sections/batch/tests.rs:230](../src/mcp/service/sections/batch/tests.rs#L230)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# P
p
## C
c
# B
b
~~~

操作列

~~~json
[
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-002"
    },
    "placement": "before",
    "content": "## X\nx\n"
  },
  {
    "type": "delete_section",
    "section": {
      "by": "id",
      "value": "s-002"
    }
  }
]
~~~

期待結果

~~~text
# P
p
## X
x
# B
b
~~~

<a id="pattern-11"></a>

### 11. 子の前へ新しい子を挿入してから親を削除

- テスト関数 `deletion_uses_current_parent_and_sibling_ranges`
- コード位置 [src/mcp/service/sections/batch/tests.rs:230](../src/mcp/service/sections/batch/tests.rs#L230)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# P
p
## C
c
# B
b
~~~

操作列

~~~json
[
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-002"
    },
    "placement": "before",
    "content": "## X\nx\n"
  },
  {
    "type": "delete_section",
    "section": {
      "by": "id",
      "value": "s-001"
    }
  }
]
~~~

期待結果

~~~text
# B
b
~~~

<a id="pattern-12"></a>

### 12. 同レベル節をafterへ挿入してから元anchorを削除

- テスト関数 `deletion_uses_current_parent_and_sibling_ranges`
- コード位置 [src/mcp/service/sections/batch/tests.rs:230](../src/mcp/service/sections/batch/tests.rs#L230)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# A
a
# B
b
~~~

操作列

~~~json
[
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-001"
    },
    "placement": "after",
    "content": "# X\nx\n"
  },
  {
    "type": "delete_section",
    "section": {
      "by": "id",
      "value": "s-001"
    }
  }
]
~~~

期待結果

~~~text
# X
x
# B
b
~~~

<a id="pattern-13"></a>

### 13. 親削除後に元の子を編集

- テスト関数 `deletion_uses_current_parent_and_sibling_ranges`
- コード位置 [src/mcp/service/sections/batch/tests.rs:230](../src/mcp/service/sections/batch/tests.rs#L230)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 エラーcode

初期本文

~~~text
# P
p
## C
c
# B
b
~~~

操作列

~~~json
[
  {
    "type": "delete_section",
    "section": {
      "by": "id",
      "value": "s-001"
    }
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-002"
    },
    "content": "wrong"
  }
]
~~~

期待結果

~~~text
not_found
~~~

<a id="pattern-14"></a>

### 14. 新設節の本文をreplace_text allで編集

- テスト関数 `inserted_text_is_editable_without_new_heading_binding`
- コード位置 [src/mcp/service/sections/batch/tests.rs:271](../src/mcp/service/sections/batch/tests.rs#L271)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# A
b
~~~

操作列

~~~json
[
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-001"
    },
    "placement": "before",
    "content": "# X\na a\n"
  },
  {
    "type": "replace_text",
    "old_text": "a",
    "new_text": "aa",
    "occurrence": "all"
  }
]
~~~

期待結果

~~~text
# X
aa aa
# A
b
~~~

<a id="pattern-15"></a>

### 15. 新設titleを同じ要求のsection selectorで参照

- テスト関数 `inserted_text_is_editable_without_new_heading_binding`
- コード位置 [src/mcp/service/sections/batch/tests.rs:271](../src/mcp/service/sections/batch/tests.rs#L271)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 エラーcode

初期本文

~~~text
# A
b
~~~

操作列

~~~json
[
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-001"
    },
    "placement": "before",
    "content": "# X\na a\n"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "title",
      "value": "X"
    },
    "content": "wrong"
  }
]
~~~

期待結果

~~~text
not_found
~~~

<a id="pattern-16"></a>

### 16. replace_textで見出し行を変更した後の元節参照

- テスト関数 `rewritten_or_hidden_heading_is_unresolvable`
- コード位置 [src/mcp/service/sections/batch/tests.rs:303](../src/mcp/service/sections/batch/tests.rs#L303)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 エラーcode

初期本文

~~~text
intro
# A
a
~~~

操作列

~~~json
[
  {
    "type": "replace_text",
    "old_text": "# A",
    "new_text": "# Z"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "wrong"
  }
]
~~~

期待結果

~~~text
invalid_input
~~~

<a id="pattern-17"></a>

### 17. コードフェンスで見出しを隠した後の元節参照

- テスト関数 `rewritten_or_hidden_heading_is_unresolvable`
- コード位置 [src/mcp/service/sections/batch/tests.rs:303](../src/mcp/service/sections/batch/tests.rs#L303)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 エラーcode

初期本文

~~~text
intro
# A
a
~~~

操作列

~~~json
[
  {
    "type": "replace_text",
    "old_text": "intro",
    "new_text": "```\nintro"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "wrong"
  }
]
~~~

期待結果

~~~text
invalid_input
~~~

<a id="pattern-18"></a>

### 18. 見出しの同一文字列置換後の元節参照

- テスト関数 `rewritten_or_hidden_heading_is_unresolvable`
- コード位置 [src/mcp/service/sections/batch/tests.rs:303](../src/mcp/service/sections/batch/tests.rs#L303)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# A
a
~~~

操作列

~~~json
[
  {
    "type": "replace_text",
    "old_text": "# A",
    "new_text": "# A"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "ok"
  }
]
~~~

期待結果

~~~text
# A
ok
~~~

<a id="pattern-19"></a>

### 19. フェンスを除去して本文を戻しても追跡不能は復活しない

- テスト関数 `rewritten_or_hidden_heading_is_unresolvable`
- コード位置 [src/mcp/service/sections/batch/tests.rs:303](../src/mcp/service/sections/batch/tests.rs#L303)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 エラーcode

初期本文

~~~text
intro
# A
a
~~~

操作列

~~~json
[
  {
    "type": "replace_text",
    "old_text": "intro",
    "new_text": "```\nintro"
  },
  {
    "type": "replace_text",
    "old_text": "```\nintro",
    "new_text": "intro"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "wrong"
  }
]
~~~

期待結果

~~~text
invalid_input
~~~

<a id="pattern-20"></a>

### 20. 削除後に同じ位置へ同名節を再作成しても元IDへ結合しない

- テスト関数 `deleted_identity_does_not_rebind_to_recreated_heading`
- コード位置 [src/mcp/service/sections/batch/tests.rs:338](../src/mcp/service/sections/batch/tests.rs#L338)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 エラーcode

初期本文

~~~text
# A
a
# B
b
~~~

操作列

~~~json
[
  {
    "type": "delete_section",
    "section": {
      "by": "id",
      "value": "s-001"
    }
  },
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-002"
    },
    "placement": "before",
    "content": "# A\na\n"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "wrong"
  }
]
~~~

期待結果

~~~text
not_found
~~~

<a id="pattern-21"></a>

### 21. 同じanchorへのbefore連続挿入

- テスト関数 `repeated_insertions_keep_original_anchor_and_current_end`
- コード位置 [src/mcp/service/sections/batch/tests.rs:365](../src/mcp/service/sections/batch/tests.rs#L365)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# A
# B
~~~

操作列

~~~json
[
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-001"
    },
    "placement": "before",
    "content": "# X\n"
  },
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-001"
    },
    "placement": "before",
    "content": "# Y\n"
  }
]
~~~

期待結果

~~~text
# X
# Y
# A
# B
~~~

<a id="pattern-22"></a>

### 22. 同じanchorへのafter連続挿入

- テスト関数 `repeated_insertions_keep_original_anchor_and_current_end`
- コード位置 [src/mcp/service/sections/batch/tests.rs:365](../src/mcp/service/sections/batch/tests.rs#L365)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 最終本文の完全一致

初期本文

~~~text
# A
# B
~~~

操作列

~~~json
[
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-001"
    },
    "placement": "after",
    "content": "# X\n"
  },
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-001"
    },
    "placement": "after",
    "content": "# Y\n"
  }
]
~~~

期待結果

~~~text
# A
# Y
# X
# B
~~~

<a id="pattern-23"></a>

### 23. 日本語Setext見出しの変更と複数節編集、末尾改行なし

- テスト関数 `utf8_setext_and_missing_final_newline_keep_identity`
- コード位置 [src/mcp/service/sections/batch/tests.rs:395](../src/mcp/service/sections/batch/tests.rs#L395)
- 初期本文の末尾改行 なし
- 結果 成功
- 照合内容 starts_withとends_with。最終本文全体の完全一致はassertしていない

初期本文

~~~text
概要
====
本文
# 次
末尾
~~~

操作列

~~~json
[
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "新",
    "title": "説明"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "再"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-002"
    },
    "content": "最後"
  }
]
~~~

期待結果

~~~text
先頭が「# 説明\n再\n# 次\n」、末尾が「最後」
~~~

<a id="pattern-24"></a>

### 24. 装飾・codeを含む見出しを初期titleで再編集

- テスト関数 `utf8_setext_and_missing_final_newline_keep_identity`
- コード位置 [src/mcp/service/sections/batch/tests.rs:395](../src/mcp/service/sections/batch/tests.rs#L395)
- 初期本文の末尾改行 なし
- 結果 成功
- 照合内容 starts_with。末尾全文はassertしていない

初期本文

~~~text
# **概要** `code`
本文
# 次
末尾
~~~

操作列

~~~json
[
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-001"
    },
    "content": "新"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "title",
      "value": "概要 code"
    },
    "content": "再"
  }
]
~~~

期待結果

~~~text
先頭が「# 概要 code\n再\n」
~~~

<a id="pattern-25"></a>

### 25. 大きい日本語本文への32回の同一置換と元節編集

- テスト関数 `representative_large_source_and_operation_array`
- コード位置 [src/mcp/service/sections/batch/tests.rs:433](../src/mcp/service/sections/batch/tests.rs#L433)
- 初期本文の末尾改行 生成される本文は末尾改行あり
- 結果 成功
- 照合内容 ends_withとmatchesの件数。初期本文は上記Rust式で生成

初期本文の生成式

~~~text
format!("# 概要\n{}\n# 次\n本文\n", "日本語".repeat(32768))
~~~

操作列

~~~json
[
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_text",
    "old_text": "日本語",
    "new_text": "日本語"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-002"
    },
    "content": "完了"
  }
]
~~~

期待結果

~~~text
末尾が「# 次\n完了\n」、日本語の出現数が32768
~~~

<a id="pattern-26"></a>

### 26. 二回の置換で元本文へ戻した要求を一回保存

- テスト関数 `batch_saves_once_and_empty_service_input_does_not_save`
- コード位置 [src/mcp/service/tests/batch.rs:84](../src/mcp/service/tests/batch.rs#L84)
- 初期本文の末尾改行 なし
- 結果 成功
- 照合内容 本文・revision・instance_idの不一致・summary

初期本文

~~~text
original
~~~

操作列

~~~json
[
  {
    "type": "replace_text",
    "old_text": "original",
    "new_text": "middle"
  },
  {
    "type": "replace_text",
    "old_text": "middle",
    "new_text": "original"
  }
]
~~~

期待結果

~~~text
本文original、revision=2、instance_id更新、summary=page edited (2 operations)
~~~

<a id="pattern-27"></a>

### 27. 先行置換成功後に後続置換が失敗した要求の未保存

- テスト関数 `batch_intermediate_and_final_validation_failures_are_atomic`
- コード位置 [src/mcp/service/tests/batch.rs:118](../src/mcp/service/tests/batch.rs#L118)
- 初期本文の末尾改行 なし
- 結果 成功
- 照合内容 エラー接頭辞、および最終FM不正要求も実行した後のDB不変

初期本文

~~~text
original
~~~

操作列

~~~json
[
  {
    "type": "replace_text",
    "old_text": "original",
    "new_text": "middle"
  },
  {
    "type": "replace_text",
    "old_text": "missing",
    "new_text": "wrong"
  }
]
~~~

期待結果

~~~text
messageはoperation 2 failed:で開始。DBは本文original・revision=1・instance_id保持
~~~

<a id="pattern-28"></a>

### 28. 二操作の一要求でUpdate監査を一件記録

- テスト関数 `edit_batch_records_one_update_audit`
- コード位置 [src/mcp/handler/tests.rs:48](../src/mcp/handler/tests.rs#L48)
- 初期本文の末尾改行 なし
- 結果 成功
- 照合内容 公開resultのrevisionと監査レコードの各値。本文自体のassertはこのテストにはない

初期本文

~~~text
before
~~~

操作列

~~~json
[
  {
    "type": "replace_text",
    "old_text": "before",
    "new_text": "middle"
  },
  {
    "type": "replace_text",
    "old_text": "middle",
    "new_text": "after"
  }
]
~~~

期待結果

~~~text
revision=2、監査行数1、operation=Update、監査revision=2、summary=page edited (2 operations)
~~~

<a id="pattern-29"></a>

### 29. HTTP tools/callで二段階置換と一revision保存

- テスト関数 `edit_page_batch_array_contract_through_transport`
- コード位置 [src/mcp/transport/tests.rs:3988](../src/mcp/transport/tests.rs#L3988)
- 初期本文の末尾改行 なし
- 結果 成功
- 照合内容 公開応答のrevision/summary、最新本文のcontains。全文一致はassertしていない

初期本文

~~~text
# page
body
~~~

操作列

~~~json
[
  {
    "type": "replace_text",
    "old_text": "body",
    "new_text": "middle"
  },
  {
    "type": "replace_text",
    "old_text": "middle",
    "new_text": "final"
  }
]
~~~

期待結果

~~~text
revisionは一増加、summary=page edited (2 operations)、最新本文にfinalを含む
~~~

<a id="pattern-30"></a>

### 30. SKILL_SETUP.md掲載JSON例をHTTP tools/callで適用

- テスト関数 `skill_setup_batch_example_uses_original_sections`
- コード位置 [src/mcp/transport/tests.rs:4044](../src/mcp/transport/tests.rs#L4044)
- 初期本文の末尾改行 あり
- 結果 成功
- 照合内容 本文完全一致、revision一増加、summary。掲載JSONを読み込みpathと競合トークンだけfixtureへ置換

初期本文

~~~text
# Overview
intro
# Usage
old
~~~

操作列

~~~json
[
  {
    "type": "insert_section",
    "anchor": {
      "by": "id",
      "value": "s-001"
    },
    "placement": "before",
    "content": "# Notice\nText\n"
  },
  {
    "type": "replace_section",
    "section": {
      "by": "id",
      "value": "s-002"
    },
    "content": "Updated body of the original second section"
  }
]
~~~

期待結果

~~~text
# Notice
Text
# Overview
intro
# Usage
Updated body of the original second section
~~~

## 保存・入力・内部防御の補助検証

以下は同じ実行に含まれた補助検証。単一編集、空入力、同時更新の挿入、障害注入等を扱う。

| テスト・条件 | 実際に確認した範囲 | コード |
| --- | --- | --- |
| batch_saves_once_and_empty_service_input_does_not_saveの空Vec | invalid_input、revision=1の維持 | [service/tests/batch.rs:84](../src/mcp/service/tests/batch.rs#L84) |
| batch_intermediate_and_final_validation_failures_are_atomicの最終front matter不正 | originalを不正front matterへ置換する要求が失敗。本文・revision・instance_idが元のまま | [service/tests/batch.rs:118](../src/mcp/service/tests/batch.rs#L118) |
| batch_final_primitive_conflict_does_not_save | 他ページと重複するprompt名を最終本文に指定すると失敗。本文original・revision=1を保持 | [service/tests/batch.rs:151](../src/mcp/service/tests/batch.rs#L151) |
| batch_save_rechecks_revision_and_instance_after_intervening_write、通常更新 | original→batchの変換後、DBをinterveningへ通常更新し、古いトークンで保存するとnot_latest_revision。最新版interveningを保持 | [service/tests/batch.rs:179](../src/mcp/service/tests/batch.rs#L179) |
| 同テスト、amend | 変換後にDBをinterveningへamendし、同じrevision・古いinstance_idで保存するとinstance_id_not_match。最新版を保持 | [service/tests/batch.rs:179](../src/mcp/service/tests/batch.rs#L179) |
| batch_post_commit_failure_preserves_saved_source | resource候補同期の障害注入でinternal_error。operation番号なし。正本savedはrevision=2へ保存済み、instance_id更新 | [service/tests/batch.rs:216](../src/mcp/service/tests/batch.rs#L216) |
| invalid_byte_boundaries_and_overflow_are_rejected | UTF-8文字「あ」の途中byte範囲1..2はinternal_error。usize::MAXへの位置加算は失敗 | [sections/batch/tests.rs:460](../src/mcp/service/sections/batch/tests.rs#L460) |
| edit_page_batch_array_contract_through_transportの不正入力 | operationの単一object・空配列を公開呼出しでJSON-RPC -32602として拒否 | [transport/tests.rs:3988](../src/mcp/transport/tests.rs#L3988) |
| edit_page_requires_nonempty_operation_array | object・空・null・入れ子・未知type・不正要素を拒否、1/2要素配列を受理。schemaのarray/minItems=1を確認 | [tools/mod.rs](../src/mcp/tools/mod.rs) |

CAS試験では、本文変換から正規保存までの間へ別更新を明示的に挿入している。並列スレッドの実行タイミングを競わせた試験ではない。commit後の同期失敗も専用faultを用いた確認である。

## この一覧で確認範囲を判断する際の補足

- 30パターンは、実装されたテスト内のケースを列挙した数であり、任意の操作列全てを網羅したという意味ではない。
- HTTP試験のサーバはテストfixture。稼働中Wikiや実際のCodex/Claudeによる編集ではない。
- 本文完全一致をassertしていないケースは、各ケースの照合内容に記載した範囲だけを実行済みの証拠とする。
- sourceの改行は読みやすい複数行で掲載している。末尾改行の有無が関係するケースは説明とassert範囲を参照する。数値のrevisionやinstance_idはfixtureで取得した値を使う。

## 関連資料

- [MCP editテスト観点](MCP_EDIT_TEST_VIEWPOINTS.md)
- [MCPツール仕様](MCP_TOOL_SPECS.md)
- [配布用スキルと利用例](../SKILL_SETUP.md)

