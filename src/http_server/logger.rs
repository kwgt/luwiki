/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! HTTPアクセスログの出力を担当するモジュール
//!

use std::future::{ready, Future, Ready};
use std::pin::Pin;
use std::time::Instant;

use actix_web::body::{BodySize, MessageBody};
use actix_web::dev::{
    forward_ready, Service, ServiceRequest, ServiceResponse, Transform,
};
use actix_web::http::{header, Version};
use actix_web::{Error, HttpMessage, HttpRequest};
use log::info;

use crate::rest_api::AuthUser;

///
/// HTTPアクセスログの出力ミドルウェア
///
pub(crate) struct AccessLogger;

impl AccessLogger {
    ///
    /// アクセスロガーの生成
    ///
    /// # 戻り値
    /// 生成したアクセスロガー
    ///
    pub(crate) fn new() -> Self {
        Self
    }
}

impl<S, B> Transform<S, ServiceRequest> for AccessLogger
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>
        + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = AccessLoggerMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AccessLoggerMiddleware { service }))
    }
}

///
/// HTTPアクセスログの出力処理を提供するミドルウェア
///
pub(crate) struct AccessLoggerMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for AccessLoggerMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>
        + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<
        dyn Future<Output = Result<Self::Response, Self::Error>> + 'static
    >>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        /*
         * リクエスト処理時間の計測開始
         */
        let start = Instant::now();
        let fut = self.service.call(req);

        Box::pin(async move {
            /*
             * リクエスト処理の完了待ち
             */
            let res = fut.await?;

            /*
             * ログ出力用の情報を構築
             */
            let request = res.request();
            let addr = request
                .peer_addr()
                .map(|addr| addr.ip().to_string())
                .unwrap_or_else(|| "-".to_string());
            let user = extract_user(request);
            let method = request.method().as_str();
            let path = request
                .uri()
                .path_and_query()
                .map(|v| v.as_str())
                .unwrap_or_else(|| request.path());
            let version = http_version(request.version());
            let status = res.status().as_u16();
            let size = body_size(res.response().body().size());
            let referer = header_value(request, header::REFERER);
            let user_agent = header_value(request, header::USER_AGENT);
            let elapsed = start.elapsed().as_secs_f64();

            info!(
                "{} {} \"{} {} {}\" {} {} \"{}\" \"{}\" {:.6}",
                addr,
                user,
                method,
                path,
                version,
                status,
                size,
                referer,
                user_agent,
                elapsed
            );

            Ok(res)
        })
    }
}

///
/// ユーザIDのログ用文字列を生成
///
/// # 引数
/// * `request` - HTTPリクエスト
///
/// # 戻り値
/// ログ出力用のユーザ文字列
///
fn extract_user(request: &HttpRequest) -> String {
    let user = request
        .extensions()
        .get::<AuthUser>()
        .map(|user| user.user_id().to_string())
        .unwrap_or_else(|| "-".to_string());
    format!("@{}", user)
}

///
/// ヘッダの値をログ用文字列として取得
///
/// # 引数
/// * `request` - HTTPリクエスト
/// * `name` - 対象のヘッダ名
///
/// # 戻り値
/// ログ出力用のヘッダ文字列
///
fn header_value(request: &HttpRequest, name: header::HeaderName) -> String {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .to_string()
}

///
/// レスポンスボディのサイズを取得
///
/// # 引数
/// * `size` - ボディサイズ情報
///
/// # 戻り値
/// ボディサイズ(バイト)
///
fn body_size(size: BodySize) -> usize {
    match size {
        BodySize::Sized(size) => usize::try_from(size).unwrap_or(0),
        _ => 0,
    }
}

///
/// HTTPバージョンの表示文字列を取得
///
/// # 引数
/// * `version` - HTTPバージョン
///
/// # 戻り値
/// ログ出力用のHTTPバージョン文字列
///
fn http_version(version: Version) -> &'static str {
    match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2.0",
        Version::HTTP_3 => "HTTP/3.0",
        _ => "HTTP/?",
    }
}
