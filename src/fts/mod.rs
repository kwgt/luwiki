/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 全文検索関連処理をまとめたモジュール
//!

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use lindera_core::mode::Mode;
use lindera_dictionary::{DictionaryConfig, DictionaryKind};
use lindera_tokenizer::tokenizer::{Tokenizer as LinderaTokenizer, TokenizerConfig};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions,
    Value, STORED, STRING, INDEXED,
};
use tantivy::tokenizer::{Token, TokenStream, Tokenizer, TextAnalyzer};
use tantivy::{doc, Index, Score, TantivyDocument, Term};

use crate::cmd_args::FtsSearchTarget;
use crate::database::DatabaseManager;
use crate::database::types::PageId;

///
/// 利用するトークナイザ種別
///
#[derive(Clone, Copy, Debug)]
pub(crate) enum TokenizerKind {
    LinderaIpadic,
}

impl TokenizerKind {
    ///
    /// トークナイザ登録名を返す
    ///
    /// # 戻り値
    /// トークナイザ登録名
    ///
    fn name(&self) -> &'static str {
        match self {
            Self::LinderaIpadic => "lindera_ipadic",
        }
    }
}

///
/// 全文検索インデックスの設定情報
///
#[derive(Clone, Debug)]
pub(crate) struct FtsIndexConfig {
    index_path: PathBuf,
    tokenizer_kind: TokenizerKind,
}

impl FtsIndexConfig {
    ///
    /// 設定情報の生成
    ///
    /// # 引数
    /// * `index_path` - インデックス格納ディレクトリのパス
    ///
    /// # 戻り値
    /// 生成した設定情報
    ///
    pub(crate) fn new(index_path: PathBuf) -> Self {
        Self {
            index_path,
            tokenizer_kind: TokenizerKind::LinderaIpadic,
        }
    }

    ///
    /// インデックス格納パスへのアクセサ
    ///
    /// # 戻り値
    /// インデックス格納パス
    ///
    pub(crate) fn index_path(&self) -> &Path {
        &self.index_path
    }
}

///
/// インデックスに登録するページ単位の文書
///
#[derive(Clone, Debug)]
pub(crate) struct FtsDocument {
    page_id: PageId,
    revision: u64,
    deleted: bool,
    is_latest: bool,
    headings: String,
    body: String,
    code: String,
}

impl FtsDocument {
    ///
    /// 文書情報の生成
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `revision` - リビジョン番号
    /// * `deleted` - 削除済みフラグ
    /// * `is_latest` - 最新リビジョン判定フラグ
    /// * `headings` - 見出しテキスト
    /// * `body` - 本文テキスト
    /// * `code` - コードブロックテキスト
    ///
    /// # 戻り値
    /// 生成した文書情報
    ///
    pub(crate) fn new(
        page_id: PageId,
        revision: u64,
        deleted: bool,
        is_latest: bool,
        headings: String,
        body: String,
        code: String,
    ) -> Self {
        Self {
            page_id,
            revision,
            deleted,
            is_latest,
            headings,
            body,
            code,
        }
    }
}

///
/// 全文検索結果の1件
///
#[derive(Clone, Debug)]
pub(crate) struct FtsSearchResult {
    page_id: PageId,
    revision: u64,
    score: Score,
    deleted: bool,
    snippet: String,
}

impl FtsSearchResult {
    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページID
    ///
    pub(crate) fn page_id(&self) -> PageId {
        self.page_id.clone()
    }

    ///
    /// リビジョン番号へのアクセサ
    ///
    /// # 戻り値
    /// リビジョン番号
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// スコアへのアクセサ
    ///
    /// # 戻り値
    /// 検索スコア
    ///
    pub(crate) fn score(&self) -> Score {
        self.score
    }

    ///
    /// 削除済みフラグへのアクセサ
    ///
    /// # 戻り値
    /// 削除済みの場合は`true`
    ///
    pub(crate) fn deleted(&self) -> bool {
        self.deleted
    }

    ///
    /// スニペットへのアクセサ
    ///
    /// # 戻り値
    /// スニペット
    ///
    pub(crate) fn snippet(&self) -> String {
        self.snippet.clone()
    }
}

///
/// Markdownから抽出した各種テキスト
///
pub(crate) struct MarkdownSections {
    pub(crate) headings: String,
    pub(crate) body: String,
    pub(crate) code: String,
}

///
/// Markdownソースから見出し/本文/コードブロックを抽出する
///
/// # 概要
/// pulldown-cmarkのイベント走査により要素ごとにテキストを収集する。
///
/// # 引数
/// * `source` - Markdownソース
///
/// # 戻り値
/// 抽出結果
///
pub(crate) fn extract_markdown_sections(source: &str) -> MarkdownSections {
    /*
     * 解析状態の初期化
     */
    let parser = Parser::new(source);
    let mut headings = String::new();
    let mut body = String::new();
    let mut code = String::new();
    let mut in_heading = false;
    let mut in_code_block = false;

    /*
     * イベント走査
     */
    for event in parser {
        /*
         * イベント種別の判定
         */
        match event {
            Event::Start(tag) => {
                if matches!(tag, Tag::Heading { .. }) {
                    in_heading = true;
                } else if matches!(tag, Tag::CodeBlock(_)) {
                    in_code_block = true;
                }
            }
            Event::End(tag) => {
                if matches!(tag, TagEnd::Heading(_)) {
                    in_heading = false;
                    push_break(&mut headings);
                } else if matches!(tag, TagEnd::CodeBlock) {
                    in_code_block = false;
                    push_break(&mut code);
                }
            }
            Event::Text(text) => {
                if in_heading {
                    push_text(&mut headings, &text);
                } else if in_code_block {
                    push_text(&mut code, &text);
                } else {
                    push_text(&mut body, &text);
                }
            }
            Event::Code(text) => {
                if in_heading {
                    push_text(&mut headings, &text);
                } else if in_code_block {
                    push_text(&mut code, &text);
                } else {
                    push_text(&mut body, &text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_heading {
                    push_break(&mut headings);
                } else if in_code_block {
                    push_break(&mut code);
                } else {
                    push_break(&mut body);
                }
            }
            _ => {}
        }
    }

    /*
     * 抽出結果の構築
     */
    MarkdownSections { headings, body, code }
}

///
/// テキストを区切り付きで追加する
///
/// # 引数
/// * `target` - 追記先のバッファ
/// * `text` - 追記する文字列
///
/// # 戻り値
/// なし
///
fn push_text(target: &mut String, text: &str) {
    if target.is_empty() {
        target.push_str(text);
        return;
    }

    if !target.ends_with(' ') && !target.ends_with('\n') {
        target.push(' ');
    }

    target.push_str(text);
}

///
/// 末尾に改行を追加する
///
/// # 引数
/// * `target` - 追記先のバッファ
///
/// # 戻り値
/// なし
///
fn push_break(target: &mut String) {
    if !target.ends_with('\n') {
        target.push('\n');
    }
}

///
/// tantivy向けlinderaトークナイザ
///
#[derive(Clone)]
struct LinderaAdapter {
    tokenizer: Arc<LinderaTokenizer>,
}

impl LinderaAdapter {
    ///
    /// linderaトークナイザの初期化
    ///
    /// # 引数
    /// * `kind` - トークナイザ種別
    ///
    /// # 戻り値
    /// 初期化済みトークナイザ
    ///
    fn new(kind: TokenizerKind) -> Result<Self> {
        match kind {
            TokenizerKind::LinderaIpadic => {
                /*
                 * トークナイザ設定の構築
                 */
                let config = TokenizerConfig {
                    dictionary: DictionaryConfig {
                        kind: Some(DictionaryKind::IPADIC),
                        path: None,
                    },
                    user_dictionary: None,
                    mode: Mode::Normal,
                };

                /*
                 * トークナイザの初期化
                 */
                let tokenizer = LinderaTokenizer::from_config(config)
                    .context("initialize lindera tokenizer")?;
                Ok(Self { tokenizer: Arc::new(tokenizer) })
            }
        }
    }
}

impl Tokenizer for LinderaAdapter {
    type TokenStream<'a> = LinderaTokenStream;

    ///
    /// トークンストリームを生成する
    ///
    /// # 概要
    /// linderaの解析結果をtantivyのトークン列に変換する。
    ///
    /// # 引数
    /// * `text` - 解析対象テキスト
    ///
    /// # 戻り値
    /// 生成したトークンストリーム
    ///
    fn token_stream<'a>(&mut self, text: &'a str) -> Self::TokenStream<'a> {
        /*
         * トークン列の生成
         */
        let tokens = match self.tokenizer.tokenize(text) {
            Ok(tokens) => tokens,
            Err(_) => Vec::new(),
        };

        /*
         * tantivyトークンへの変換
         */
        let mut output = Vec::with_capacity(tokens.len());
        for (pos, token) in tokens.into_iter().enumerate() {
            let mut out = Token::default();
            out.text = token.text.to_string();
            out.offset_from = token.byte_start;
            out.offset_to = token.byte_end;
            out.position = pos;
            out.position_length = 1;
            output.push(out);
        }

        /*
         * ストリームの生成
         */
        LinderaTokenStream { tokens: output, index: 0 }
    }
}

///
/// linderaトークン列のアダプタ
///
struct LinderaTokenStream {
    tokens: Vec<Token>,
    index: usize,
}

impl TokenStream for LinderaTokenStream {
    ///
    /// 次のトークンへ進める
    ///
    /// # 戻り値
    /// 次トークンが存在する場合は`true`
    ///
    fn advance(&mut self) -> bool {
        if self.index < self.tokens.len() {
            self.index += 1;
            true
        } else {
            false
        }
    }

    ///
    /// 現在トークンへの参照を返す
    ///
    /// # 戻り値
    /// 現在トークン
    ///
    fn token(&self) -> &Token {
        &self.tokens[self.index - 1]
    }

    ///
    /// 現在トークンへの可変参照を返す
    ///
    /// # 戻り値
    /// 現在トークン
    ///
    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.index - 1]
    }
}

///
/// 全文検索スキーマ情報
///
struct FtsSchema {
    schema: Schema,
    page_id: Field,
    revision: Field,
    deleted: Field,
    is_latest: Field,
    headings: Field,
    body: Field,
    code: Field,
}

impl FtsSchema {
    ///
    /// 新規インデックス用のスキーマ生成
    ///
    /// # 概要
    /// 検索対象フィールドと保存フィールドを定義する。
    ///
    /// # 引数
    /// * `tokenizer_name` - トークナイザ名
    ///
    /// # 戻り値
    /// 構築済みスキーマ
    ///
    fn build(tokenizer_name: &str) -> Result<Self> {
        /*
         * スキーマ構築の初期化
         */
        let mut builder = Schema::builder();

        /*
         * テキストフィールド設定
         */
        let text_indexing = TextFieldIndexing::default()
            .set_tokenizer(tokenizer_name)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let text_options = TextOptions::default()
            .set_indexing_options(text_indexing)
            .set_stored();

        /*
         * 各フィールドの定義
         */
        let page_id = builder.add_text_field("page_id", STRING | STORED);
        let revision = builder.add_u64_field("revision", INDEXED | STORED);
        let deleted = builder.add_bool_field("deleted", INDEXED | STORED);
        let is_latest = builder.add_bool_field("is_latest", INDEXED | STORED);
        let headings = builder.add_text_field("headings", text_options.clone());
        let body = builder.add_text_field("body", text_options.clone());
        let code = builder.add_text_field("code", text_options);

        let schema = builder.build();

        /*
         * スキーマの返却
         */
        Ok(Self {
            schema,
            page_id,
            revision,
            deleted,
            is_latest,
            headings,
            body,
            code,
        })
    }

    ///
    /// 既存インデックスからスキーマを復元
    ///
    /// # 概要
    /// インデックス内のフィールド定義を参照して構築する。
    ///
    /// # 引数
    /// * `index` - インデックス
    ///
    /// # 戻り値
    /// 参照に成功したスキーマ
    ///
    fn from_index(index: &Index) -> Result<Self> {
        /*
         * スキーマの取得と検証
         */
        let schema = index.schema();
        let page_id = schema.get_field("page_id")
            .with_context(|| "page_id field missing")?;
        let revision = schema.get_field("revision")
            .with_context(|| "revision field missing")?;
        let deleted = schema.get_field("deleted")
            .with_context(|| "deleted field missing")?;
        let is_latest = schema.get_field("is_latest")
            .with_context(|| "is_latest field missing")?;
        let headings = schema.get_field("headings")
            .with_context(|| "headings field missing")?;
        let body = schema.get_field("body")
            .with_context(|| "body field missing")?;
        let code = schema.get_field("code")
            .with_context(|| "code field missing")?;

        /*
         * スキーマの返却
         */
        Ok(Self {
            schema,
            page_id,
            revision,
            deleted,
            is_latest,
            headings,
            body,
            code,
        })
    }
}

///
/// 全文検索インデックスの操作管理
///
struct FtsIndexManager {
    index: Index,
    schema: FtsSchema,
}

impl FtsIndexManager {
    ///
    /// インデックスを開く（未作成なら生成）
    ///
    /// # 概要
    /// ディレクトリを作成し、存在しない場合は新規インデックスを作成する。
    ///
    /// # 引数
    /// * `config` - インデックス設定
    ///
    /// # 戻り値
    /// インデックスマネージャ
    ///
    fn open(config: &FtsIndexConfig) -> Result<Self> {
        /*
         * ディレクトリ準備
         */
        let index_path = config.index_path();
        fs::create_dir_all(index_path)?;

        /*
         * インデックスのオープンまたは生成
         */
        let index = match Index::open_in_dir(index_path) {
            Ok(index) => index,
            Err(_) => {
                let schema = FtsSchema::build(config.tokenizer_kind.name())?;
                let index = Index::create_in_dir(index_path, schema.schema.clone())?;
                index
            }
        };

        /*
         * トークナイザ登録とスキーマ取得
         */
        register_tokenizer(&index, config.tokenizer_kind)?;
        let schema = FtsSchema::from_index(&index)?;

        Ok(Self { index, schema })
    }

    ///
    /// インデックスを新規作成する（既存は削除）
    ///
    /// # 概要
    /// 既存インデックスを削除し、空のインデックスを作成する。
    ///
    /// # 引数
    /// * `config` - インデックス設定
    ///
    /// # 戻り値
    /// インデックスマネージャ
    ///
    fn create(config: &FtsIndexConfig) -> Result<Self> {
        /*
         * 既存ディレクトリの削除
         */
        let index_path = config.index_path();
        if index_path.exists() {
            fs::remove_dir_all(index_path)
                .with_context(|| format!("remove {}", index_path.display()))?;
        }
        fs::create_dir_all(index_path)?;

        /*
         * インデックスの生成
         */
        let schema = FtsSchema::build(config.tokenizer_kind.name())?;
        let index = Index::create_in_dir(index_path, schema.schema.clone())?;
        register_tokenizer(&index, config.tokenizer_kind)?;

        Ok(Self { index, schema })
    }

    ///
    /// インデックスを再構築する
    ///
    /// # 概要
    /// 受け取った文書一覧をインデックスに登録する。
    ///
    /// # 引数
    /// * `docs` - 文書一覧
    ///
    /// # 戻り値
    /// 処理に成功した場合は`Ok(())`
    ///
    fn rebuild(&self, docs: &[FtsDocument]) -> Result<()> {
        /*
         * ライタの準備
         */
        let mut writer = self.index.writer::<TantivyDocument>(50_000_000)?;

        /*
         * 文書の追加
         */
        for item in docs {
            let doc = doc!(
                self.schema.page_id => item.page_id.to_string(),
                self.schema.revision => item.revision,
                self.schema.deleted => item.deleted,
                self.schema.is_latest => item.is_latest,
                self.schema.headings => item.headings.clone(),
                self.schema.body => item.body.clone(),
                self.schema.code => item.code.clone(),
            );
            writer.add_document(doc)?;
        }

        /*
         * コミット
         */
        writer.commit()?;
        Ok(())
    }

    ///
    /// 検索を実行する
    ///
    /// # 概要
    /// 対象フィールドを指定し検索式で検索を行う。
    ///
    /// # 引数
    /// * `target` - 検索対象フィールド
    /// * `expression` - 検索式
    /// * `with_deleted` - 削除済みを含める場合は`true`
    /// * `all_revision` - 全リビジョン対象の場合は`true`
    ///
    /// # 戻り値
    /// 検索結果一覧
    ///
    fn search(
        &self,
        target: FtsSearchTarget,
        expression: &str,
        with_deleted: bool,
        all_revision: bool,
    ) -> Result<Vec<FtsSearchResult>> {
        /*
         * 検索対象の決定
         */
        let field = match target {
            FtsSearchTarget::Headings => self.schema.headings,
            FtsSearchTarget::Body => self.schema.body,
            FtsSearchTarget::Code => self.schema.code,
        };

        /*
         * クエリの構築
         */
        let query_parser = QueryParser::for_index(&self.index, vec![field]);
        let query = query_parser.parse_query(expression)?;
        let query = if with_deleted && all_revision {
            query
        } else {
            /*
             * フィルタ条件の組み立て
             */
            let mut clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> =
                Vec::new();
            clauses.push((Occur::Must, query));

            if !with_deleted {
                let term =
                    Term::from_field_bool(self.schema.deleted, false);
                let query = TermQuery::new(term, IndexRecordOption::Basic);
                clauses.push((Occur::Must, Box::new(query)));
            }

            if !all_revision {
                let term =
                    Term::from_field_bool(self.schema.is_latest, true);
                let query = TermQuery::new(term, IndexRecordOption::Basic);
                clauses.push((Occur::Must, Box::new(query)));
            }

            Box::new(BooleanQuery::new(clauses))
        };
        let reader = self.index.reader()?;
        let searcher = reader.searcher();

        /*
         * スニペット生成器の準備
         */
        let mut snippet_generator =
            tantivy::snippet::SnippetGenerator::create(
                &searcher,
                &*query,
                field,
            )?;
        snippet_generator.set_max_num_chars(200);

        /*
         * 検索結果の構築
         */
        let top_docs = searcher.search(&query, &TopDocs::with_limit(100))?;
        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            /*
             * 文書の取得
             */
            let doc: TantivyDocument = searcher.doc(doc_address)?;

            let page_id = doc
                .get_first(self.schema.page_id)
                .and_then(|value| value.as_str())
                .ok_or_else(|| anyhow!("page_id missing"))?;
            let page_id = PageId::from_string(page_id)
                .map_err(|err| anyhow!("invalid page_id: {}", err))?;

            let revision = doc
                .get_first(self.schema.revision)
                .and_then(|value| value.as_u64())
                .ok_or_else(|| anyhow!("revision missing"))?;

            let deleted = doc
                .get_first(self.schema.deleted)
                .and_then(|value| value.as_bool())
                .unwrap_or(false);

            let snippet = snippet_generator.snippet_from_doc(&doc).to_html();

            /*
             * 結果の追加
             */
            results.push(FtsSearchResult {
                page_id,
                revision,
                score,
                deleted,
                snippet,
            });
        }

        Ok(results)
    }

    ///
    /// 特定ページの文書を置き換える
    ///
    /// # 概要
    /// ページIDで既存文書を削除し、新しい文書群を登録する。
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    /// * `docs` - 登録する文書一覧
    ///
    /// # 戻り値
    /// 処理に成功した場合は`Ok(())`
    ///
    fn replace_page_docs(
        &self,
        page_id: &PageId,
        docs: &[FtsDocument],
    ) -> Result<()> {
        /*
         * ライタの準備
         */
        let mut writer = self.index.writer::<TantivyDocument>(50_000_000)?;

        /*
         * 既存文書の削除
         */
        let term = Term::from_field_text(
            self.schema.page_id,
            &page_id.to_string(),
        );
        writer.delete_term(term);

        /*
         * 文書の追加
         */
        for item in docs {
            let doc = doc!(
                self.schema.page_id => item.page_id.to_string(),
                self.schema.revision => item.revision,
                self.schema.deleted => item.deleted,
                self.schema.is_latest => item.is_latest,
                self.schema.headings => item.headings.clone(),
                self.schema.body => item.body.clone(),
                self.schema.code => item.code.clone(),
            );
            writer.add_document(doc)?;
        }

        /*
         * コミット
         */
        writer.commit()?;
        Ok(())
    }

    ///
    /// 特定ページの文書を削除する
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    ///
    /// # 戻り値
    /// 処理に成功した場合は`Ok(())`
    ///
    fn delete_page_docs(&self, page_id: &PageId) -> Result<()> {
        /*
         * ライタの準備
         */
        let mut writer = self.index.writer::<TantivyDocument>(50_000_000)?;

        /*
         * 文書の削除
         */
        let term = Term::from_field_text(
            self.schema.page_id,
            &page_id.to_string(),
        );
        writer.delete_term(term);

        /*
         * コミット
         */
        writer.commit()?;
        Ok(())
    }

    ///
    /// セグメントの強制マージを実行する
    ///
    /// # 概要
    /// 現在のセグメントをまとめてマージする。
    ///
    /// # 戻り値
    /// 処理に成功した場合は`Ok(())`
    ///
    fn merge(&self) -> Result<()> {
        /*
         * マージ対象の収集
         */
        let mut writer = self.index.writer::<TantivyDocument>(50_000_000)?;
        let reader = self.index.reader()?;
        let segment_ids: Vec<_> = reader
            .searcher()
            .segment_readers()
            .iter()
            .map(|reader| reader.segment_id())
            .collect();

        /*
         * マージの実行
         */
        if !segment_ids.is_empty() {
            writer.merge(&segment_ids);
            writer.commit()?;
            writer.wait_merging_threads()?;
        }

        Ok(())
    }
}

///
/// インデックスにトークナイザを登録する
///
/// # 引数
/// * `index` - 登録対象インデックス
/// * `kind` - トークナイザ種別
///
/// # 戻り値
/// 処理に成功した場合は`Ok(())`
///
fn register_tokenizer(index: &Index, kind: TokenizerKind) -> Result<()> {
    let tokenizer = LinderaAdapter::new(kind)?;
    let analyzer = TextAnalyzer::from(tokenizer);
    index.tokenizers().register(kind.name(), analyzer);
    Ok(())
}

///
/// 全文検索インデックスの再構築
///
/// # 引数
/// * `config` - インデックス設定
/// * `docs` - 文書一覧
///
/// # 戻り値
/// 処理に成功した場合は`Ok(())`
///
pub(crate) fn rebuild_index(
    config: &FtsIndexConfig,
    docs: &[FtsDocument],
) -> Result<()> {
    let manager = FtsIndexManager::create(config)?;
    manager.rebuild(docs)
}

///
/// 全文検索の実行
///
/// # 引数
/// * `config` - インデックス設定
/// * `target` - 検索対象フィールド
/// * `expression` - 検索式
/// * `with_deleted` - 削除済みを含める場合は`true`
/// * `all_revision` - 全リビジョン対象の場合は`true`
///
/// # 戻り値
/// 検索結果一覧
///
pub(crate) fn search_index(
    config: &FtsIndexConfig,
    target: FtsSearchTarget,
    expression: &str,
    with_deleted: bool,
    all_revision: bool,
) -> Result<Vec<FtsSearchResult>> {
    let manager = FtsIndexManager::open(config)?;
    manager.search(target, expression, with_deleted, all_revision)
}

///
/// ページ単位でインデックスを更新する
///
/// # 引数
/// * `config` - インデックス設定
/// * `manager` - データベースマネージャ
/// * `page_id` - 対象ページID
/// * `deleted` - 削除済みフラグ
///
/// # 戻り値
/// 処理に成功した場合は`Ok(())`
///
pub(crate) fn reindex_page(
    config: &FtsIndexConfig,
    manager: &DatabaseManager,
    page_id: &PageId,
    deleted: bool,
) -> Result<()> {
    /*
     * 登録文書の構築
     */
    let docs = build_documents_for_page(manager, page_id, deleted)?;
    let index_manager = FtsIndexManager::open(config)?;

    /*
     * インデックスの更新
     */
    if docs.is_empty() {
        return index_manager.delete_page_docs(page_id);
    }

    index_manager.replace_page_docs(page_id, &docs)
}

///
/// ページ単位でインデックスを削除する
///
/// # 引数
/// * `config` - インデックス設定
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// 処理に成功した場合は`Ok(())`
///
pub(crate) fn delete_page_index(
    config: &FtsIndexConfig,
    page_id: &PageId,
) -> Result<()> {
    let index_manager = FtsIndexManager::open(config)?;
    index_manager.delete_page_docs(page_id)
}

///
/// 指定ページのインデックスを更新する
///
/// # 引数
/// * `config` - インデックス設定
/// * `manager` - データベースマネージャ
/// * `page_ids` - 更新対象のページID一覧
/// * `deleted` - 削除済みフラグ
///
/// # 戻り値
/// 処理に成功した場合は`Ok(())`
///
pub(crate) fn update_pages_index(
    config: &FtsIndexConfig,
    manager: &DatabaseManager,
    page_ids: &[PageId],
    deleted: bool,
) -> Result<()> {
    /*
     * インデックス更新の実行
     */
    for page_id in page_ids {
        reindex_page(config, manager, page_id, deleted)?;
    }

    Ok(())
}

///
/// 指定ページのインデックスを削除する
///
/// # 引数
/// * `config` - インデックス設定
/// * `page_ids` - 削除対象のページID一覧
///
/// # 戻り値
/// 処理に成功した場合は`Ok(())`
///
pub(crate) fn delete_pages_index(
    config: &FtsIndexConfig,
    page_ids: &[PageId],
) -> Result<()> {
    /*
     * インデックス削除の実行
     */
    for page_id in page_ids {
        delete_page_index(config, page_id)?;
    }

    Ok(())
}

///
/// 指定パス配下のページID一覧を収集する
///
/// # 概要
/// 起点パス配下のページ情報から削除済みとドラフトを除外し、
/// ページIDを収集する。
///
/// # 引数
/// * `manager` - データベースマネージャ
/// * `base_path` - 起点ページのパス
///
/// # 戻り値
/// ページID一覧
///
pub(crate) fn collect_page_ids_by_path_prefix(
    manager: &DatabaseManager,
    base_path: &str,
) -> Result<Vec<PageId>> {
    /*
     * 検索パスの準備
     */
    let prefix = if base_path.ends_with('/') {
        base_path.to_string()
    } else {
        format!("{}/", base_path)
    };

    /*
     * ページ情報の収集
     */
    let mut page_ids = Vec::new();
    for entry in manager.list_pages()? {
        if entry.deleted() || entry.is_draft() {
            continue;
        }

        let path = entry.path();
        if path == base_path || path.starts_with(&prefix) {
            page_ids.push(entry.id());
        }
    }

    Ok(page_ids)
}

///
/// セグメント強制マージの実行
///
/// # 引数
/// * `config` - インデックス設定
///
/// # 戻り値
/// 処理に成功した場合は`Ok(())`
///
pub(crate) fn merge_index(config: &FtsIndexConfig) -> Result<()> {
    let manager = FtsIndexManager::open(config)?;
    manager.merge()
}

///
/// ページ単位の登録文書を構築する
///
/// # 概要
/// ページインデックスとソースを参照し、検索文書を生成する。
///
/// # 引数
/// * `manager` - データベースマネージャ
/// * `page_id` - 対象ページID
/// * `deleted` - 削除済みフラグ
///
/// # 戻り値
/// 登録対象の文書一覧
///
fn build_documents_for_page(
    manager: &DatabaseManager,
    page_id: &PageId,
    deleted: bool,
) -> Result<Vec<FtsDocument>> {
    /*
     * ページインデックスの取得
     */
    let index = match manager.get_page_index_by_id(page_id)? {
        Some(index) => index,
        None => return Err(anyhow!("page not found")),
    };

    if index.is_draft() {
        return Ok(Vec::new());
    }

    /*
     * 文書の構築
     */
    let latest = index.latest();
    let mut docs = Vec::new();
    for entry in manager.list_page_source_entries_by_id(page_id)? {
        let source = entry.source().source();
        let sections = extract_markdown_sections(&source);
        let is_latest = entry.revision() == latest;
        docs.push(FtsDocument::new(
            entry.page_id(),
            entry.revision(),
            deleted,
            is_latest,
            sections.headings,
            sections.body,
            sections.code,
        ));
    }

    Ok(docs)
}
