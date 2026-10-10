/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! REST要求の監査補足と公開結果の分類を定義するモジュール
//!
// 固定resource境界と全handlerの確定対象補足を共通sinkへ接続する。

use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::{Method, StatusCode};
use actix_web::middleware::Next;
use actix_web::{Error, HttpMessage, HttpRequest, HttpResponse, web};
use chrono::Utc;
use tracing::warn;

use crate::audit::model::{
    AuditOperation, AuditRecord, AuditResult, RestAuditOperation,
};
use crate::audit::AuditSink;
use crate::auth::AuthContext;
use crate::database::types::{TokenId, UserId};
use crate::http_server::app_state::AppState;
use crate::wiki_path::NormalizedWikiPath;

///
/// 登録時に確定したREST resourceの操作記述
///
#[derive(Clone, Copy)]
pub(crate) struct RestResourceDescriptor {
    /// 固定route template（/api scope内の相対表記）
    pub(crate) route: &'static str,
    /// 当該resourceに登録する閉じた操作集合
    operations: &'static [RestAuditOperation],
}

impl RestResourceDescriptor {
    ///
    /// 登録済みmethodに対応する固定監査操作を選ぶ
    ///
    /// # 引数
    /// * `method` - HTTP要求method
    ///
    /// # 戻り値
    /// 対応する固定操作。未対応methodはNone。
    ///
    pub(crate) fn operation(
        self, method: &Method,
    ) -> Option<RestAuditOperation> {
        self.operations.iter().copied().find(|operation| {
            operation.as_str().split_once(' ')
                .is_some_and(|(fixed, _)| fixed == method.as_str())
        })
    }
}

///
/// 登録済み30操作をまとめた固定resource descriptor
///
pub(crate) const REST_RESOURCES: [RestResourceDescriptor; 22] = [
    RestResourceDescriptor {
        route: "/hello",
        operations: &[
            RestAuditOperation::Hello,
        ],
    },
    RestResourceDescriptor {
        route: "/pages",
        operations: &[
            RestAuditOperation::CreatePage,
            RestAuditOperation::ListPages,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/deleted",
        operations: &[
            RestAuditOperation::DeletedPages,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/id",
        operations: &[
            RestAuditOperation::PageId,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/short",
        operations: &[
            RestAuditOperation::PageShortByPath,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/template",
        operations: &[
            RestAuditOperation::Templates,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/search",
        operations: &[
            RestAuditOperation::Search,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/{page_id}/source",
        operations: &[
            RestAuditOperation::PageSource,
            RestAuditOperation::PutPageSource,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/{page_id}/meta",
        operations: &[
            RestAuditOperation::PageMeta,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/{page_id}/parent",
        operations: &[
            RestAuditOperation::PageParent,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/{page_id}/short",
        operations: &[
            RestAuditOperation::PageShort,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/{page_id}/path",
        operations: &[
            RestAuditOperation::PagePath,
            RestAuditOperation::MovePage,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/{page_id}/revision",
        operations: &[
            RestAuditOperation::RevisePage,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/{page_id}/assets",
        operations: &[
            RestAuditOperation::PageAssets,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/{page_id}/assets/{file_name}",
        operations: &[
            RestAuditOperation::UploadPageAsset,
            RestAuditOperation::ResolvePageAsset,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/{page_id}/lock",
        operations: &[
            RestAuditOperation::AcquireLock,
            RestAuditOperation::RenewLock,
            RestAuditOperation::ReadLock,
            RestAuditOperation::ReleaseLock,
        ],
    },
    RestResourceDescriptor {
        route: "/pages/{page_id}",
        operations: &[
            RestAuditOperation::DeletePage,
        ],
    },
    RestResourceDescriptor {
        route: "/assets",
        operations: &[
            RestAuditOperation::UploadAsset,
            RestAuditOperation::ResolveAsset,
        ],
    },
    RestResourceDescriptor {
        route: "/assets/{asset_id}/data",
        operations: &[
            RestAuditOperation::AssetData,
        ],
    },
    RestResourceDescriptor {
        route: "/assets/{asset_id}/meta",
        operations: &[
            RestAuditOperation::AssetMeta,
        ],
    },
    RestResourceDescriptor {
        route: "/assets/{asset_id}",
        operations: &[
            RestAuditOperation::DeleteAsset,
        ],
    },
    RestResourceDescriptor {
        route: "/users/me",
        operations: &[
            RestAuditOperation::Me,
        ],
    },
];

///
/// 同じ公開操作内の確定分岐
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RestAuditAction {
    /// ページ移動
    Rename,
    /// ページ復元
    Restore,
    /// 履歴巻き戻し
    Rollback,
    /// 履歴圧縮
    Compaction,
}

impl RestAuditAction {
    ///
    /// 固定分岐名を返す
    ///
    /// # 戻り値
    /// サーバ定義の分岐名。
    ///
    fn as_str(self) -> &'static str {
        match self {
            Self::Rename => "rename",
            Self::Restore => "restore",
            Self::Rollback => "rollback",
            Self::Compaction => "compaction",
        }
    }
}

///
/// パス拒否が発生した側
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RestDeniedSide {
    /// 移動元
    Source,
    /// 移動先
    Destination,
    /// 親候補
    Parent,
}

impl RestDeniedSide {
    ///
    /// 固定拒否側名を返す
    ///
    /// # 戻り値
    /// サーバ定義の拒否側名。
    ///
    fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Destination => "destination",
            Self::Parent => "parent",
        }
    }
}

///
/// 同txn又は業務後処理で確認した失敗段階
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RestCommitPhase {
    /// DB未変更が確認されたcommit前失敗
    PreCommit,
    /// commit成功確認後の業務後処理失敗
    PostCommit,
    /// 新規ファイル回収失敗（DB commit状態のみを保持）
    Cleanup {
        /// DB commit成功を確認したか
        committed: bool,
    },
    /// 回収失敗を確認したがDB commit結果は未確定
    CleanupUnconfirmed,
}

impl RestCommitPhase {
    ///
    /// 固定段階名と確認済みDB commit状態を返す
    ///
    /// # 戻り値
    /// 固定段階名、確認済みDB commit状態（未知はNone）。
    ///
    fn parts(self) -> (&'static str, Option<bool>) {
        match self {
            Self::PreCommit => ("pre_commit", Some(false)),
            Self::PostCommit => ("post_commit", Some(true)),
            Self::Cleanup { committed } => ("cleanup", Some(committed)),
            Self::CleanupUnconfirmed => ("cleanup", None),
        }
    }
}

///
/// 要求内でのみ保持する型付きの確定監査情報
///
#[derive(Clone, Debug, Default)]
pub(crate) struct RestAuditMetadata {
    /// 同snapshotで解決・認可した対象（raw入力不可）
    pub(crate) target: Option<NormalizedWikiPath>,
    /// 実取得又はcommit／指定操作対象が確定した単一revision
    pub(crate) revision: Option<u64>,
    /// handlerで解釈が確定した分岐
    pub(crate) action: Option<RestAuditAction>,
    /// 実返却集合の件数（候補集合の再計算不可）
    pub(crate) count: Option<usize>,
    /// 実返却時に確定した後続候補の有無
    pub(crate) has_more: Option<bool>,
    /// 解釈が確定したamend指定
    pub(crate) amend: Option<bool>,
    /// 解釈が確定した再帰指定
    pub(crate) recursive: Option<bool>,
    /// 同txnで解決・認可した最終移動先
    pub(crate) destination: Option<NormalizedWikiPath>,
    /// 型付きguardで確定した結果
    pub(crate) result: Option<AuditResult>,
    /// パス認可で確認した拒否側
    pub(crate) denied_side: Option<RestDeniedSide>,
    /// 業務層で事実を確認した失敗段階
    pub(crate) phase: Option<RestCommitPhase>,
}

///
/// 最終HTTP状態と操作契約へ照合した監査項目
///
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RestAuditFields {
    /// 最終公開結果
    pub(crate) result: AuditResult,
    /// 操作が許す確定対象path
    pub(crate) target_path: Option<String>,
    /// 操作が許す確定revision
    pub(crate) revision: Option<u64>,
    /// 固定語彙と型付き確定値だけによる要約
    pub(crate) summary: Option<String>,
}

impl RestAuditMetadata {
    ///
    /// 操作別契約と最終HTTP状態に合わせて監査項目を確定する
    ///
    /// # 引数
    /// * `operation` - 登録済み固定操作
    /// * `status` - 最終HTTP状態
    ///
    /// # 戻り値
    /// 未知情報を省略した監査項目。
    ///
    /// # 概要
    /// statusを統合し、操作に許された対象・補足だけを取り出す。
    ///
    pub(crate) fn finish(
        &self,
        operation: RestAuditOperation,
        status: StatusCode,
    ) -> RestAuditFields {
        /*
         * 最終500系を優先し、typed deny等はその他の失敗で保持する
         */
        let fallback = status_result(status);
        let result = if status.is_server_error()
            || fallback == AuditResult::Success
        {
            fallback
        } else {
            self.result.filter(|value| !matches!(value,
                AuditResult::Success | AuditResult::Unsupported
            ))
                .unwrap_or(fallback)
        };

        /*
         * 操作ごとの対象・revision許可項目に限定する
         */
        let target_path = if matches!(operation,
            RestAuditOperation::Hello | RestAuditOperation::Me
            | RestAuditOperation::Templates | RestAuditOperation::Search
        ) {
            None
        } else {
            self.target.as_ref().map(|path| path.as_str().to_string())
        };
        let revision = if target_path.is_some() && matches!(operation,
            RestAuditOperation::PageSource | RestAuditOperation::PutPageSource
            | RestAuditOperation::PageMeta | RestAuditOperation::MovePage
            | RestAuditOperation::RevisePage
        ) {
            self.revision
        } else {
            None
        };

        RestAuditFields {
            result,
            target_path,
            revision,
            summary: self.summary(operation, result, status),
        }
    }

    ///
    /// 操作に対応する確定actionだけを選ぶ
    ///
    /// # 引数
    /// * `operation` - 固定操作
    ///
    /// # 戻り値
    /// 当該操作に属する分岐、未確定時はNone。
    ///
    fn selected_action(
        &self,
        operation: RestAuditOperation,
    ) -> Option<RestAuditAction> {
        match (operation, self.action) {
            (RestAuditOperation::MovePage,
                Some(RestAuditAction::Rename | RestAuditAction::Restore))
            | (RestAuditOperation::RevisePage,
                Some(RestAuditAction::Rollback | RestAuditAction::Compaction))
                => self.action,
            _ => None,
        }
    }

    ///
    /// 固定語彙と当該操作に許された補足だけから要約を作る
    ///
    /// # 引数
    /// * `operation` - 固定操作
    /// * `result` - 最終分類
    /// * `status` - 最終HTTP状態
    ///
    /// # 戻り値
    /// 補足があれば要約、なければNone。
    ///
    /// # 概要
    /// 共通失敗、分岐、操作別補足、確認済みcommit段階を順に選ぶ。
    ///
    fn summary(
        &self,
        operation: RestAuditOperation,
        result: AuditResult,
        status: StatusCode,
    ) -> Option<String> {
        /*
         * 失敗分類と操作に属する確定分岐
         */
        let success = result == AuditResult::Success;
        let mut parts = Vec::new();
        if !success {
            let failure = if status == StatusCode::PAYLOAD_TOO_LARGE {
                "payload_too_large"
            } else {
                result.as_str()
            };
            parts.push(format!("failure={failure}"));
        }
        let action = self.selected_action(operation);
        if let Some(action) = action {
            parts.push(format!("action={}", action.as_str()));
        }

        /*
         * 当該操作で確定した入力・返却値だけを補足する
         */
        self.operation_summary(operation, result, action, &mut parts);
        if !success {
            self.failure_summary(operation, result, &mut parts);
        }
        if parts.is_empty() { None } else { Some(parts.join(" ")) }
    }

    ///
    /// 操作固有の入力・返却値を要約へ加える
    ///
    /// # 引数
    /// * `operation` - 固定操作
    /// * `result` - 最終公開結果
    /// * `action` - 操作に属する分岐
    /// * `parts` - 固定補足の出力先
    ///
    /// # 戻り値
    /// なし。
    ///
    /// # 概要
    /// 成功の返却件数と、失敗でも確定済みの指定値を区別する。
    ///
    fn operation_summary(
        &self,
        operation: RestAuditOperation,
        result: AuditResult,
        action: Option<RestAuditAction>,
        parts: &mut Vec<String>,
    ) {
        /*
         * 返却件数とページ一覧の継続有無は成功時だけ補足する
         */
        let success = result == AuditResult::Success;
        if success && matches!(operation,
            RestAuditOperation::ListPages | RestAuditOperation::DeletedPages
            | RestAuditOperation::Templates | RestAuditOperation::Search
            | RestAuditOperation::PageAssets
        ) {
            if let Some(count) = self.count {
                parts.push(format!("count={count}"));
            }
            if operation == RestAuditOperation::ListPages {
                if let Some(has_more) = self.has_more {
                    parts.push(format!("has_more={has_more}"));
                }
            }
        }

        /*
         * 入力解釈の確定値と、変更確定済みの最終移動先
         */
        if operation == RestAuditOperation::PutPageSource {
            if let Some(amend) = self.amend {
                parts.push(format!("amend={amend}"));
            }
        }
        if operation == RestAuditOperation::DeletePage {
            if let Some(recursive) = self.recursive {
                parts.push(format!("recursive={recursive}"));
            }
        }
        if operation == RestAuditOperation::MovePage && action.is_some()
            && (success || (result == AuditResult::InternalError
                && self.phase == Some(RestCommitPhase::PostCommit)))
        {
            if let Some(destination) = &self.destination {
                parts.push(format!("destination={}", destination.as_str()));
            }
        }
    }

    ///
    /// 確認済みの拒否側・更新失敗段階を要約へ加える
    ///
    /// # 引数
    /// * `operation` - 固定操作
    /// * `result` - 最終失敗分類
    /// * `parts` - 固定補足の出力先
    ///
    /// # 戻り値
    /// なし。
    ///
    /// # 概要
    /// 拒否側は対応するpath拒否だけ、phaseは対象更新だけに限定する。
    ///
    fn failure_summary(
        &self,
        operation: RestAuditOperation,
        result: AuditResult,
        parts: &mut Vec<String>,
    ) {
        /*
         * 親候補と移動のsource/destination拒否を区別する
         */
        if result == AuditResult::PathPrefixDenied {
            let side = match (operation, self.denied_side) {
                (RestAuditOperation::PageParent, Some(RestDeniedSide::Parent))
                    => self.denied_side,
                (RestAuditOperation::MovePage,
                    Some(RestDeniedSide::Source | RestDeniedSide::Destination))
                    => self.denied_side,
                _ => None,
            };
            if let Some(side) = side {
                parts.push(format!("denied_side={}", side.as_str()));
            }
        }

        /*
         * phaseは更新操作で事実が明示された場合だけ使う
         */
        if matches!(operation,
            RestAuditOperation::CreatePage | RestAuditOperation::PutPageSource
            | RestAuditOperation::MovePage | RestAuditOperation::RevisePage
            | RestAuditOperation::UploadPageAsset
            | RestAuditOperation::DeletePage
            | RestAuditOperation::UploadAsset | RestAuditOperation::DeleteAsset
        ) {
            if let Some(phase) = self.phase.filter(|phase| {
                *phase == RestCommitPhase::PreCommit
                    || result == AuditResult::InternalError
            }) {
                let (name, committed) = phase.parts();
                parts.push(format!("phase={name}"));
                if let Some(committed) = committed {
                    parts.push(format!("committed={committed}"));
                }
            }
        }
    }
}

///
/// HTTP状態だけで分類できる既定結果を返す
///
/// # 引数
/// * `status` - 最終HTTP状態
///
/// # 戻り値
/// 型付き補足がない場合の結果分類。
///
/// # 概要
/// 公開HTTP状態から判別できる失敗だけを固定分類へ写像する。
///
pub(crate) fn status_result(status: StatusCode) -> AuditResult {
    /*
     * 通常成功と定義済みキャッシュ・redirect成功
     */
    if status.is_success() || matches!(status.as_u16(), 302 | 304) {
        return AuditResult::Success;
    }
    /*
     * HTTP状態だけから特定できる固定失敗分類
     */
    match status.as_u16() {
        400 | 413 | 415 => AuditResult::InvalidInput,
        403 => AuditResult::OperationDenied,
        404 => AuditResult::NotFound,
        409 => AuditResult::Conflict,
        410 => AuditResult::Gone,
        423 => AuditResult::Locked,
        _ => AuditResult::InternalError,
    }
}

///
/// 後続のresource入口で空の要求内補足を準備する
///
/// # 引数
/// * `req` - 認証済み要求
///
/// # 戻り値
/// なし。本文・資格情報を読み取らない。
///
pub(crate) fn prepare_metadata(req: &HttpRequest) {
    req.extensions_mut().insert(RestAuditMetadata::default());
}

///
/// 後続handlerから確定した最小情報を補足する
///
/// # 引数
/// * `req` - 要求
/// * `update` - 型付き情報だけを更新する処理
///
/// # 戻り値
/// 境界で準備済みならtrue。未接続要求には情報を新設しない。
///
pub(crate) fn update_metadata(
    req: &HttpRequest,
    update: impl FnOnce(&mut RestAuditMetadata),
) -> bool {
    let mut extensions = req.extensions_mut();
    if let Some(metadata) = extensions.get_mut::<RestAuditMetadata>() {
        update(metadata);
        true
    } else {
        false
    }
}

///
/// 後続境界で補足の所有値を取得する
///
/// # 引数
/// * `req` - 要求
///
/// # 戻り値
/// 補足のsnapshot。未準備なら空の補足。
///
pub(crate) fn metadata_snapshot(req: &HttpRequest) -> RestAuditMetadata {
    req.extensions().get::<RestAuditMetadata>().cloned().unwrap_or_default()
}

///
/// Next Err応答化後も既存expiry headerのskipを維持する内部印
///
pub(super) struct RestNextErrorResponse;

///
/// 境界入口で複製した認証済み主体
///
struct RestAuditActor {
    /// 検証済みの不変ユーザID
    user_id: UserId,
    /// Bearer ID、BasicはNone
    token_id: Option<TokenId>,
    /// 接続元IP、取得できない場合はNone
    address: Option<IpAddr>,
}

impl RestAuditActor {
    ///
    /// 認証補足を終了時lookupに依存せず複製する
    ///
    /// # 引数
    /// * `req` - 認証済み要求
    ///
    /// # 戻り値
    /// 検証済みactor。未検証contextはNone。
    ///
    fn from_request(req: &HttpRequest) -> Option<Self> {
        let auth = req.extensions().get::<AuthContext>().cloned()?;
        Some(Self {
            user_id: auth.actor_id()?.clone(),
            token_id: auth.token_id().cloned(),
            address: req.peer_addr().map(|peer| peer.ip()),
        })
    }
}

///
/// 認証後resource境界で公開要求を一度だけ監査へ渡す
///
/// # 引数
/// * `req` - 認証済み要求
/// * `next` - extractorとhandlerを含む後続service
/// * `descriptor` - 登録時に固定した操作集合
///
/// # 戻り値
/// 元応答、又はResponseErrorが定義する元のエラー応答。
///
/// # 概要
/// actorを複製してから後続を実行し最終応答に基づき一度投入する。
///
pub(crate) async fn request_boundary<B>(
    req: ServiceRequest,
    next: Next<B>,
    descriptor: RestResourceDescriptor,
) -> Result<ServiceResponse<BoxBody>, Error>
where
    B: MessageBody + 'static,
{
    /*
     * 未登録methodと未検証主体を監査対象へ拡張しない
     */
    let Some(operation) = descriptor.operation(req.method()) else {
        return next.call(req).await.map(ServiceResponse::map_into_boxed_body);
    };
    let request = req.request().clone();
    let Some(actor) = RestAuditActor::from_request(&request) else {
        warn!("REST audit skipped: verified actor unavailable");
        return next.call(req).await.map(ServiceResponse::map_into_boxed_body);
    };

    /*
     * 同じsink参照だけを複製しguardを後続awaitへ持ち越さない
     */
    let sink = request.app_data::<web::Data<Arc<RwLock<AppState>>>>()
        .and_then(|state| state.read().ok()?.audit_sink());
    prepare_metadata(&request);

    /*
     * Next ErrもResponseError本来の応答へ変換して統一する
     */
    let response = match next.call(req).await {
        Ok(response) => response.map_into_boxed_body(),
        Err(error) => {
            let mut response = HttpResponse::from_error(error);
            response.extensions_mut().insert(RestNextErrorResponse);
            ServiceResponse::new(request.clone(), response)
        }
    };

    /*
     * 最終statusと確定metadataだけから一件を投入する
     */
    enrich_metadata(&request, operation, response.status());
    let fields = metadata_snapshot(&request).finish(
        operation, response.status(),
    );
    let record = AuditRecord::new(
        AuditOperation::Rest(operation), actor.user_id, actor.token_id,
        actor.address, fields.target_path, fields.result,
        Utc::now(), fields.summary, fields.revision,
    );
    submit_record(sink, record);
    Ok(response)
}

///
/// 共通sinkへ一度投入を試み失敗時も業務応答を変えない
///
/// # 引数
/// * `sink` - 入口で複製した共有sink
/// * `record` - 確定レコード
///
/// # 戻り値
/// なし。再試行せず固定診断だけを出す。
///
/// # 概要
/// DB／state guardを保持せず同期audit lockだけで投入する。
///
fn submit_record(sink: Option<Arc<RwLock<AuditSink>>>, record: AuditRecord) {
    /*
     * G5接続前のsink未設定を記録成功と扱わない
     */
    let Some(sink) = sink else {
        warn!("REST audit skipped: sink unavailable");
        return;
    };

    /*
     * エラー本文や内部pathを診断へ転記しない
     */
    match sink.write() {
        Ok(mut sink) => {
            if sink.record(record).is_err() {
                warn!("REST audit record failed");
            }
        }
        Err(_) => warn!("REST audit record failed: sink lock unavailable"),
    }
}

///
/// 同snapshotで取得したページ対象を要求内へ保持する
///
/// # 引数
/// * `req` - 要求
/// * `page` - DBの認可済み参照値
///
/// # 戻り値
/// なし。監査補足だけの失敗で業務結果を変更しない。
///
pub(crate) fn remember_page(
    req: &HttpRequest,
    page: &crate::database::AuthorizedPage,
) {
    if let Ok(target) = crate::database::PageAccessTarget::from_page(page) {
        req.extensions_mut().insert(target);
    }
}

///
/// 同txnのlock対象indexをrevisionなしで保持する
///
/// # 引数
/// * `req` - 要求
/// * `index` - 同txnの認可済みindex
///
/// # 戻り値
/// なし。
///
pub(crate) fn remember_index(
    req: &HttpRequest,
    index: &crate::database::types::PageIndex,
) {
    if let Ok(target) = crate::database::PageAccessTarget::from_index(index) {
        req.extensions_mut().insert(target);
    }
}

///
/// 入力解決で成功した正規形だけを補足する
///
/// # 引数
/// * `req` - 要求
/// * `path` - 正規形検証結果。raw文字列は受け取らない
///
/// # 戻り値
/// なし。
///
pub(crate) fn remember_path(
    req: &HttpRequest,
    path: anyhow::Result<NormalizedWikiPath>,
) {
    if let Ok(path) = path {
        update_metadata(req, |metadata| metadata.target = Some(path));
    }
}

///
/// DBの型付きerror contextを要求内へ保存する
///
/// # 引数
/// * `req` - 要求
/// * `error` - 既存DB結果
///
/// # 戻り値
/// なし。エラー本文や秘密値は取得しない。
///
/// # 概要
/// 同txnで確定した最小情報だけを所有値として保存する。
///
pub(crate) fn remember_error(req: &HttpRequest, error: &anyhow::Error) {
    /*
     * 参照・更新の既知対象は既存contextから複製する
     */
    if let Some(target) =
        error.downcast_ref::<crate::database::PageAccessTarget>()
    {
        req.extensions_mut().insert(target.clone());
    }
    if let Some(target) =
        error.downcast_ref::<crate::database::AssetAccessTarget>()
    {
        req.extensions_mut().insert(target.clone());
    }
    if let Some(outcome) =
        error.downcast_ref::<crate::database::MutationOutcome>()
    {
        req.extensions_mut().insert(outcome.clone());
    }

    /*
     * uploadの業務段階は型付き事実だけから設定する
     */
    let pre_commit =
        error.downcast_ref::<crate::database::AssetUploadPreCommit>().is_some();
    if error.downcast_ref::<crate::database::AssetFileRecoveryError>().is_some()
    {
        update_metadata(req, |metadata| {
            metadata.phase = Some(if pre_commit {
                RestCommitPhase::Cleanup { committed: false }
            } else { RestCommitPhase::CleanupUnconfirmed });
        });
    } else if pre_commit {
        update_metadata(req, |metadata| {
            metadata.phase = Some(RestCommitPhase::PreCommit);
        });
    }

    /*
     * 認可拒否側と指定revisionの検証事実を残す
     */
    if let Some(denied) =
        error.downcast_ref::<crate::database::PathPrefixDenied>()
    {
        req.extensions_mut().insert(denied.clone());
    }
    if let Some(side) =
        error.downcast_ref::<crate::database::MoveDeniedSide>() {
        req.extensions_mut().insert(*side);
    }
    if let Some(target) =
        error.downcast_ref::<crate::database::RevisionAccessTarget>()
    {
        update_metadata(req, |metadata| {
            metadata.revision = Some(target.revision);
        });
    }
}

///
/// 操作別に既存確定contextを監査補足へ選択する
///
/// # 引数
/// * `req` - 要求
/// * `operation` - 固定操作
/// * `status` - 最終状態
///
/// # 戻り値
/// なし。別DB lookupとレスポンス解析は行わない。
///
/// # 概要
/// 同snapshot値、commit事実、失敗側を操作契約どおり採用する。
///
fn enrich_metadata(
    req: &HttpRequest,
    operation: RestAuditOperation,
    status: StatusCode,
) {
    /*
     * snapshot参照とowner対象は最小値だけ取り出す
     */
    let extensions = req.extensions();
    let page = extensions.get::<crate::database::PageAccessTarget>().cloned();
    let asset = extensions.get::<crate::database::AssetAccessTarget>().cloned();
    let outcome = extensions.get::<crate::database::MutationOutcome>().cloned();
    let denied = extensions.get::<crate::database::PathPrefixDenied>().cloned();
    let side = extensions.get::<crate::database::MoveDeniedSide>().copied();
    drop(extensions);

    /*
     * target/revisionを操作ごとに選びcommit前の候補revを排除する
     */
    update_metadata(req, |metadata| {
        if let Some(page) = page {
            metadata.target = Some(page.path);
            metadata.revision = page.revision;
        }
        if let Some(asset) = asset {
            metadata.target = NormalizedWikiPath::from_stored(&asset.path).ok();
        }
        if let Some(outcome) = outcome {
            let path = if operation == RestAuditOperation::MovePage {
                &outcome.source_path
            } else { &outcome.path };
            metadata.target = NormalizedWikiPath::from_stored(path).ok();
            if matches!(operation,
                RestAuditOperation::PutPageSource | RestAuditOperation::MovePage
            ) && (outcome.committed || outcome.no_op) && !outcome.cleanup_only
            {
                metadata.revision = outcome.revision;
            }
            if operation == RestAuditOperation::MovePage {
                metadata.destination =
                    NormalizedWikiPath::from_stored(&outcome.path).ok();
            }
            if status.is_server_error() && outcome.committed {
                if outcome.cleanup_only {
                    metadata.phase = Some(RestCommitPhase::Cleanup {
                        committed: true,
                    });
                } else if !outcome.no_op || outcome.post_processing_failure {
                    metadata.phase = Some(RestCommitPhase::PostCommit);
                }
            }
        }

        /*
         * 拒否対象を成功元pathより優先し、親404の分類を守る
         */
        if let Some(denied) = denied {
            metadata.target =
                NormalizedWikiPath::from_stored(&denied.path).ok();
            metadata.result = Some(AuditResult::PathPrefixDenied);
            if denied.parent_candidate {
                metadata.denied_side = Some(RestDeniedSide::Parent);
            }
            if let Some(side) = side {
                metadata.denied_side = Some(match side {
                    crate::database::MoveDeniedSide::Source =>
                        RestDeniedSide::Source,
                    crate::database::MoveDeniedSide::Destination =>
                        RestDeniedSide::Destination,
                });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use actix_web::test::TestRequest;

    use super::*;

    ///
    /// 最終statusの分類とtyped親拒否・500優先を検証する
    ///
    /// # 概要
    /// 確定した型付き情報と既存公開契約をfixtureで照合する。
    ///
    /// # 注記
    /// 公開status一覧、親404、最終500を同じ補足に適用する。
    ///
    #[test]
    fn rest_audit_status_fallback_and_typed_failures() {
        /*
         * statusだけで分かる分類を確認する
         */
        for (code, expected) in [
            (200, AuditResult::Success), (201, AuditResult::Success),
            (204, AuditResult::Success), (302, AuditResult::Success),
            (304, AuditResult::Success), (400, AuditResult::InvalidInput),
            (413, AuditResult::InvalidInput), (415, AuditResult::InvalidInput),
            (403, AuditResult::OperationDenied), (404, AuditResult::NotFound),
            (409, AuditResult::Conflict), (410, AuditResult::Gone),
            (423, AuditResult::Locked), (500, AuditResult::InternalError),
            (503, AuditResult::InternalError),
        ] {
            assert_eq!(
                status_result(StatusCode::from_u16(code).unwrap()), expected,
            );
        }

        /*
         * 親候補拒否は404からNotFoundへ誤分類しない
         */
        let metadata = RestAuditMetadata {
            result: Some(AuditResult::PathPrefixDenied),
            denied_side: Some(RestDeniedSide::Parent),
            target: Some(NormalizedWikiPath::parse("/parent").unwrap()),
            ..Default::default()
        };
        let parent = metadata.finish(
            RestAuditOperation::PageParent, StatusCode::NOT_FOUND,
        );
        assert_eq!(parent.result, AuditResult::PathPrefixDenied);
        assert_eq!(parent.target_path.as_deref(), Some("/parent"));
        assert_eq!(parent.summary.as_deref(),
            Some("failure=path_prefix_denied denied_side=parent"));

        /*
         * scopeとReadOnlyは一般403のfallbackから区別する
         */
        for result in [AuditResult::ScopeDenied, AuditResult::ReadOnlyDenied] {
            let typed = RestAuditMetadata {
                result: Some(result), ..Default::default()
            };
            let fields = typed.finish(
                RestAuditOperation::PutPageSource, StatusCode::FORBIDDEN,
            );
            assert_eq!(fields.result, result);
            assert_eq!(fields.summary, Some(format!(
                "failure={}", result.as_str(),
            )));
        }

        /*
         * 最終500ではtyped denyもInternalErrorへ統合する
         */
        let failed = metadata.finish(
            RestAuditOperation::PageParent, StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(failed.result, AuditResult::InternalError);
        assert_eq!(failed.summary.as_deref(), Some("failure=internal_error"));
    }

    ///
    /// 操作別の対象・revision・件数の許可項目を検証する
    ///
    /// # 概要
    /// 確定した型付き情報と既存公開契約をfixtureで照合する。
    ///
    /// # 注記
    /// 全30操作へ過剰補足を渡し不要情報の出力を拒否する。
    ///
    #[test]
    fn rest_audit_operation_fields_are_allowlisted() {
        /*
         * 意図的に多数の情報を与え当該操作の項目だけを選ばせる
         */
        let metadata = RestAuditMetadata {
            target: Some(NormalizedWikiPath::parse("/safe").unwrap()),
            revision: Some(7), count: Some(3), has_more: Some(false),
            amend: Some(true), recursive: Some(false),
            destination: Some(NormalizedWikiPath::parse("/dest").unwrap()),
            action: Some(RestAuditAction::Rename),
            phase: Some(RestCommitPhase::PostCommit),
            ..Default::default()
        };
        for operation in RestAuditOperation::ALL {
            let fields = metadata.finish(operation, StatusCode::OK);
            let no_target = matches!(operation,
                RestAuditOperation::Hello | RestAuditOperation::Me
                | RestAuditOperation::Templates | RestAuditOperation::Search
            );
            assert_eq!(fields.target_path.is_none(), no_target);
            let revision = matches!(operation,
                RestAuditOperation::PageSource
                | RestAuditOperation::PutPageSource
                | RestAuditOperation::PageMeta
                | RestAuditOperation::MovePage
                | RestAuditOperation::RevisePage
            );
            assert_eq!(fields.revision, revision.then_some(7));
            let expected = match operation {
                RestAuditOperation::ListPages =>
                    Some("count=3 has_more=false"),
                RestAuditOperation::DeletedPages
                | RestAuditOperation::Templates
                | RestAuditOperation::Search
                | RestAuditOperation::PageAssets => Some("count=3"),
                RestAuditOperation::PutPageSource => Some("amend=true"),
                RestAuditOperation::DeletePage => Some("recursive=false"),
                RestAuditOperation::MovePage =>
                    Some("action=rename destination=/dest"),
                _ => None,
            };
            assert_eq!(fields.summary.as_deref(), expected, "{operation:?}");
        }
    }

    ///
    /// 未確定値をnull又は省略とし失敗phaseを推測しない
    ///
    /// # 概要
    /// 確定した型付き情報と既存公開契約をfixtureで照合する。
    ///
    /// # 注記
    /// 空補足と入力不正・不明500を全操作で確認する。
    ///
    #[test]
    fn rest_audit_unknown_values_are_not_invented() {
        for operation in RestAuditOperation::ALL {
            /*
             * 未接続要求の情報は空のままにする
             */
            let metadata = RestAuditMetadata::default();
            let success = metadata.finish(operation, StatusCode::OK);
            assert_eq!(success.target_path, None);
            assert_eq!(success.revision, None);
            assert_eq!(success.summary, None);

            /*
             * エラー本文やcommit段階を推測せず固定分類だけ返す
             */
            let failure = metadata.finish(
                operation, StatusCode::INTERNAL_SERVER_ERROR,
            );
            assert_eq!(failure.summary.as_deref(),
                Some("failure=internal_error"));
            let too_large = metadata.finish(
                operation, StatusCode::PAYLOAD_TOO_LARGE,
            );
            assert_eq!(too_large.result, AuditResult::InvalidInput);
            assert_eq!(too_large.summary.as_deref(),
                Some("failure=payload_too_large"));
        }
    }

    ///
    /// 分岐・拒否側・確定commit情報の失敗補足を検証する
    ///
    /// # 概要
    /// 確定した型付き情報と既存公開契約をfixtureで照合する。
    ///
    /// # 注記
    /// 移動pre/post、revision操作、upload回収失敗を比較する。
    ///
    #[test]
    fn rest_audit_commit_and_action_contracts() {
        /*
         * 移動先は失敗時のpost_commitが確定した場合だけ補足する
         */
        let mut metadata = RestAuditMetadata {
            action: Some(RestAuditAction::Restore),
            destination: Some(NormalizedWikiPath::parse("/new").unwrap()),
            target: Some(NormalizedWikiPath::parse("/old").unwrap()),
            revision: Some(8),
            result: Some(AuditResult::PathPrefixDenied),
            denied_side: Some(RestDeniedSide::Destination),
            ..Default::default()
        };
        let denied = metadata.finish(
            RestAuditOperation::MovePage, StatusCode::FORBIDDEN,
        );
        assert_eq!(denied.summary.as_deref(),
            Some(concat!(
                "failure=path_prefix_denied action=restore ",
                "denied_side=destination",
            )));
        metadata.phase = Some(RestCommitPhase::PostCommit);
        let still_denied = metadata.finish(
            RestAuditOperation::MovePage, StatusCode::FORBIDDEN,
        );
        assert_eq!(still_denied.summary, denied.summary);
        let failed = metadata.finish(
            RestAuditOperation::MovePage, StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(failed.revision, Some(8));
        assert_eq!(failed.summary.as_deref(),
            Some(concat!(
                "failure=internal_error action=restore destination=/new ",
                "phase=post_commit committed=true",
            )));

        /*
         * rollback/compactionは確定actionだけを扱い移動先を出さない
         */
        for action in [RestAuditAction::Rollback, RestAuditAction::Compaction] {
            metadata.action = Some(action);
            let fields = metadata.finish(
                RestAuditOperation::RevisePage, StatusCode::OK,
            );
            assert_eq!(fields.summary, Some(format!(
                "action={}", action.as_str(),
            )));
        }

        /*
         * 回収と未変更確認は別の固定段階に限定する
         */
        metadata.phase = Some(RestCommitPhase::Cleanup { committed: false });
        let cleanup = metadata.finish(
            RestAuditOperation::UploadAsset, StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(cleanup.summary.as_deref(),
            Some("failure=internal_error phase=cleanup committed=false"));
        metadata.phase = Some(RestCommitPhase::PreCommit);
        metadata.result = None;
        let pre = metadata.finish(
            RestAuditOperation::PutPageSource, StatusCode::BAD_REQUEST,
        );
        assert_eq!(pre.summary.as_deref(),
            Some("failure=invalid_input phase=pre_commit committed=false"));
    }

    ///
    /// 要求補足を分離しraw入力・資格情報・本文を取得しない
    ///
    /// # 概要
    /// 確定した型付き情報と既存公開契約をfixtureで照合する。
    ///
    /// # 注記
    /// 秘密を含む要求を用意し型付き補足だけをsnapshotへ保存する。
    ///
    #[test]
    fn rest_audit_metadata_is_request_local_and_secret_free() {
        /*
         * raw URL/body/headerは補足準備とsnapshotの入力にしない
         */
        let secret = "must_never_appear";
        let req = TestRequest::default()
            .uri("/api/pages/secret?query=must_never_appear")
            .insert_header(("Authorization", "Bearer must_never_appear"))
            .insert_header(("X-Lock-Authentication", secret))
            .set_payload(secret)
            .to_http_request();
        let other = TestRequest::default().to_http_request();
        assert!(!update_metadata(&req, |_| panic!("not prepared")));
        prepare_metadata(&req);
        assert!(update_metadata(&req, |metadata| {
            metadata.count = Some(2);
        }));
        let fields = metadata_snapshot(&req).finish(
            RestAuditOperation::Search, StatusCode::OK,
        );
        assert_eq!(fields.summary.as_deref(), Some("count=2"));
        assert!(!format!("{fields:?}").contains(secret));
        assert_eq!(metadata_snapshot(&other).count, None);

        /*
         * 正規化型は空白/control付きraw文字列を受け付けない
         */
        assert!(NormalizedWikiPath::parse("/unsafe path").is_err());
        assert!(NormalizedWikiPath::from_stored("/unsafe/../path").is_err());
    }
}


#[cfg(test)]
mod projection_tests {
    use actix_web::test::TestRequest;

    use super::*;
    use crate::database::{MutationOutcome, PageAccessTarget};
    use crate::database::types::PageId;

    ///
    /// 保持する既存MutationOutcomeのfixtureを生成する
    ///
    /// # 引数
    /// * `committed` - 確認済みcommit状態
    ///
    /// # 戻り値
    /// 本文や資格情報を含まない確定補助。
    ///
    fn outcome(committed: bool) -> MutationOutcome {
        MutationOutcome {
            asset_id: None, page_id: PageId::new(),
            source_path: "/known".into(), path: "/known".into(),
            revision: Some(99), instance_id: None,
            affected_page_ids: Vec::new(), committed,
            post_processing_failure: false,
            no_op: false, cleanup_only: false,
        }
    }

    ///
    /// commit未確認の候補Vとphaseを公開監査へ捏造しない
    ///
    /// # 概要
    /// commit Err contextと確定commit後失敗を別々に投影する。
    ///
    /// # 注記
    /// committed=falseだけではpre_commitにならずVはNone。
    ///
    #[test]
    fn rest_projection_unconfirmed_commit_omits_candidate_revision() {
        /*
         * native commit Err相当の未確認contextを投影する
         */
        let req = TestRequest::default().to_http_request();
        prepare_metadata(&req);
        let error = anyhow::anyhow!("private native commit failure")
            .context(outcome(false));
        remember_error(&req, &error);
        enrich_metadata(&req, RestAuditOperation::PutPageSource,
            StatusCode::INTERNAL_SERVER_ERROR);
        let fields = metadata_snapshot(&req).finish(
            RestAuditOperation::PutPageSource,
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(fields.target_path.as_deref(), Some("/known"));
        assert_eq!(fields.revision, None);
        assert_eq!(fields.summary.as_deref(), Some("failure=internal_error"));

        /*
         * 成功確認済みcommit後の500は確定Vとphaseを保持する
         */
        req.extensions_mut().insert(outcome(true));
        enrich_metadata(&req, RestAuditOperation::PutPageSource,
            StatusCode::INTERNAL_SERVER_ERROR);
        let fields = metadata_snapshot(&req).finish(
            RestAuditOperation::PutPageSource,
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(fields.revision, Some(99));
        assert_eq!(fields.summary.as_deref(),
            Some("failure=internal_error phase=post_commit committed=true"));
    }

    ///
    /// no-op/合法cleanup commit/cleanup未確認を区別する
    ///
    /// # 概要
    /// 要求更新commitを推測せず確認済み副作用を固定段階へ保持する。
    ///
    /// # 注記
    /// cleanup500はcleanup true、未確認cleanupはbooleanを省略する。
    ///
    #[test]
    fn rest_projection_cleanup_and_noop_preserve_only_confirmed_facts() {
        /*
         * no-opは既知latestだけを保持し新commitを推測しない
         */
        let req = TestRequest::default().to_http_request();
        prepare_metadata(&req);
        let mut value = outcome(false);
        value.no_op = true;
        req.extensions_mut().insert(value);
        enrich_metadata(&req, RestAuditOperation::MovePage,
            StatusCode::NO_CONTENT);
        assert_eq!(metadata_snapshot(&req).finish(
            RestAuditOperation::MovePage, StatusCode::NO_CONTENT,
        ).revision, Some(99));

        /*
         * 合法cleanup commitの失敗を要求更新post_commitと混同しない
         */
        prepare_metadata(&req);
        let mut value = outcome(true);
        value.cleanup_only = true;
        req.extensions_mut().insert(value);
        enrich_metadata(&req, RestAuditOperation::PutPageSource,
            StatusCode::INTERNAL_SERVER_ERROR);
        let fields = metadata_snapshot(&req).finish(
            RestAuditOperation::PutPageSource,
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(fields.revision, None);
        assert_eq!(fields.summary.as_deref(),
            Some("failure=internal_error phase=cleanup committed=true"));

        /*
         * 実commit失敗後の回収失敗はDBのtrue/falseを推測しない
         */
        let error = anyhow::anyhow!("private failure")
            .context(crate::database::AssetFileRecoveryError {
                reason: "private filesystem diagnostic".into(),
            });
        remember_error(&req, &error);
        let fields = metadata_snapshot(&req).finish(
            RestAuditOperation::UploadAsset,
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(fields.summary.as_deref(),
            Some("failure=internal_error phase=cleanup"));
    }

    ///
    /// 最小read contextの実取得Vと未確定Vを区別する
    ///
    /// # 概要
    /// source未取得の既知対象でもpathを保持しraw指定Vを作らない。
    ///
    /// # 注記
    /// user lookup等のread Err contextを同じ補足入口へ渡す。
    ///
    #[test]
    fn rest_projection_read_error_keeps_only_known_revision() {
        let req = TestRequest::default().to_http_request();
        prepare_metadata(&req);
        for revision in [None, Some(3)] {
            /*
             * source取得有無の確定値だけをcontextへ渡す
             */
            let error = anyhow::anyhow!("private read error")
                .context(PageAccessTarget {
                    path: NormalizedWikiPath::parse("/known").unwrap(),
                    revision,
                });
            remember_error(&req, &error);
            enrich_metadata(&req, RestAuditOperation::PageSource,
                StatusCode::INTERNAL_SERVER_ERROR);
            let fields = metadata_snapshot(&req).finish(
                RestAuditOperation::PageSource,
                StatusCode::INTERNAL_SERVER_ERROR,
            );
            assert_eq!(fields.target_path.as_deref(), Some("/known"));
            assert_eq!(fields.revision, revision);
            assert_eq!(fields.summary.as_deref(),
                Some("failure=internal_error"));
        }
    }

    ///
    /// 指定revisionの確定contextをOutcome.latestから独立させる
    ///
    /// # 概要
    /// commit未確認でも既存validator成立済みの指定Vだけを保持する。
    ///
    /// # 注記
    /// 指定V=2と候補latest=99を同時に与え取り違えを検出する。
    ///
    #[test]
    fn rest_projection_specified_revision_is_independent_of_candidate_latest() {
        /*
         * 確定指定Vと未commit候補latestを同じerror chainへ渡す
         */
        let req = TestRequest::default().to_http_request();
        prepare_metadata(&req);
        let error = anyhow::anyhow!("private commit failure")
            .context(outcome(false))
            .context(crate::database::RevisionAccessTarget { revision: 2 });
        remember_error(&req, &error);
        enrich_metadata(&req, RestAuditOperation::RevisePage,
            StatusCode::INTERNAL_SERVER_ERROR);
        let fields = metadata_snapshot(&req).finish(
            RestAuditOperation::RevisePage, StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(fields.target_path.as_deref(), Some("/known"));
        assert_eq!(fields.revision, Some(2));
        assert_eq!(fields.summary.as_deref(), Some("failure=internal_error"));
    }

}
