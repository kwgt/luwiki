/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! コンフィギュレーション情報の定義
//!

use std::default::Default;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::{default_log_path, default_db_path, default_assets_path, LogLevel};
use crate::cmd_args::{
    AssetListSortMode,
    LockListSortMode,
    PageListSortMode,
    UserListSortMode,
};

const DEFAULT_FRONTEND_UI_FONT: &str = "sans-serif";
const DEFAULT_FRONTEND_MD_FONT_SANS: &str = "sans-serif";
const DEFAULT_FRONTEND_MD_FONT_SERIF: &str = "serif";
const DEFAULT_FRONTEND_MD_FONT_MONO: &str = "monospace";
const DEFAULT_FRONTEND_MD_CODE_FONT: &str = "monospace";

///
/// コンフィギュレーションデータを集約する構造体
///
#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Config {
    #[serde(skip)]
    config_path: Option<PathBuf>,

    /// グローバルオプションに対する情報
    global: Option<GlobalInfo>,

    /// runサブコマンド用の設定
    run: Option<RunInfo>,

    /// user listサブコマンド用の設定
    user: Option<UserSection>,

    /// pageサブコマンド用の設定
    page: Option<PageSection>,

    /// lock listサブコマンド用の設定
    lock: Option<LockSection>,

    /// assetサブコマンド用の設定
    asset: Option<AssetSection>,

    /// frontend設定
    frontend: Option<FrontendSection>,
}

impl Config {
    ///
    /// グローバル設定のログレベルを更新
    ///
    pub(super) fn set_log_level(&mut self, level: LogLevel) {
        let global = self.ensure_global();
        global.log_level = Some(level);
    }

    ///
    /// グローバル設定のログ出力先を更新
    ///
    pub(super) fn set_log_output(&mut self, path: PathBuf) {
        let global = self.ensure_global();
        global.log_output = Some(path);
    }

    ///
    /// グローバル設定のデータベースパスを更新
    ///
    pub(super) fn set_db_path(&mut self, path: PathBuf) {
        let global = self.ensure_global();
        global.db_path = Some(path);
    }

    ///
    /// グローバル設定のアセットパスを更新
    ///
    pub(super) fn set_assets_path(&mut self, path: PathBuf) {
        let global = self.ensure_global();
        global.assets_path = Some(path);
    }

    ///
    /// runサブコマンドのバインドアドレスを更新
    ///
    pub(super) fn set_run_bind_addr(&mut self, addr: String) {
        let run = self.ensure_run();
        run.bind_addr = Some(addr);
    }

    ///
    /// runサブコマンドのバインドポートを更新
    ///
    pub(super) fn set_run_bind_port(&mut self, port: u16) {
        let run = self.ensure_run();
        run.bind_port = Some(port);
    }

    ///
    /// user listサブコマンドのソートモードを更新
    ///
    pub(super) fn set_user_list_sort_mode(&mut self, mode: UserListSortMode,) {
        let list = self.ensure_user_list();
        list.sort_mode = Some(mode);
    }

    ///
    /// user listサブコマンドの逆順指定を更新
    ///
    pub(super) fn set_user_list_reverse_sort(&mut self, reverse: bool) {
        let list = self.ensure_user_list();
        list.reverse_sort = Some(reverse);
    }

    ///
    /// page listサブコマンドのソートモードを更新
    ///
    pub(super) fn set_page_list_sort_mode(&mut self, mode: PageListSortMode,) {
        let list = self.ensure_page_list();
        list.sort_mode = Some(mode);
    }

    ///
    /// page listサブコマンドの逆順指定を更新
    ///
    pub(super) fn set_page_list_reverse_sort(&mut self, reverse: bool) {
        let list = self.ensure_page_list();
        list.reverse_sort = Some(reverse);
    }

    ///
    /// page listサブコマンドの詳細表示指定を更新
    ///
    pub(super) fn set_page_list_long_info(&mut self, long_info: bool) {
        let list = self.ensure_page_list();
        list.long_info = Some(long_info);
    }

    ///
    /// page addサブコマンドのデフォルトユーザを更新
    ///
    pub(super) fn set_page_add_default_user(&mut self, user_name: String) {
        let add = self.ensure_page_add();
        add.default_user = Some(user_name);
    }

    ///
    /// page undeleteサブコマンドのアセット復旧有無を更新
    ///
    pub(super) fn set_page_undelete_with_assets(&mut self, with_assets: bool) {
        let undelete = self.ensure_page_undelete();
        undelete.with_assets = Some(with_assets);
    }

    ///
    /// lock listサブコマンドのソートモードを更新
    ///
    pub(super) fn set_lock_list_sort_mode(&mut self, mode: LockListSortMode,) {
        let list = self.ensure_lock_list();
        list.sort_mode = Some(mode);
    }

    ///
    /// lock listサブコマンドの逆順指定を更新
    ///
    pub(super) fn set_lock_list_reverse_sort(&mut self, reverse: bool) {
        let list = self.ensure_lock_list();
        list.reverse_sort = Some(reverse);
    }

    ///
    /// lock listサブコマンドの詳細表示指定を更新
    ///
    pub(super) fn set_lock_list_long_info(&mut self, long_info: bool) {
        let list = self.ensure_lock_list();
        list.long_info = Some(long_info);
    }

    ///
    /// asset addサブコマンドのデフォルトユーザを更新
    ///
    pub(super) fn set_asset_add_default_user(&mut self, user_name: String) {
        let add = self.ensure_asset_add();
        add.default_user = Some(user_name);
    }

    ///
    /// asset listサブコマンドのソートモードを更新
    ///
    pub(super) fn set_asset_list_sort_mode(
        &mut self,
        mode: AssetListSortMode,
    ) {
        let list = self.ensure_asset_list();
        list.sort_mode = Some(mode);
    }

    ///
    /// asset listサブコマンドの逆順指定を更新
    ///
    pub(super) fn set_asset_list_reverse_sort(&mut self, reverse: bool) {
        let list = self.ensure_asset_list();
        list.reverse_sort = Some(reverse);
    }

    ///
    /// asset listサブコマンドの詳細表示指定を更新
    ///
    pub(super) fn set_asset_list_long_info(&mut self, long_info: bool) {
        let list = self.ensure_asset_list();
        list.long_info = Some(long_info);
    }
    ///
    /// データベースファイルへのパスへのアクセサ
    ///
    /// # 戻り値
    /// データベースファイルパスが設定されている場合はパス情報を`Some()`でラップ
    /// して返す。
    ///
    pub(super) fn db_path(&self) -> Option<PathBuf> {
        self.global
            .as_ref()
            .and_then(|global| global.db_path.as_ref())
            .map(|path| self.resolve_path(path))
    }

    ///
    /// ログレベルへのアクセサ
    ///
    pub(super) fn log_level(&self) -> Option<LogLevel> {
        self.global
            .as_ref()
            .and_then(|global| global.log_level)
    }

    ///
    /// ログ出力先へのアクセサ
    ///
    pub(super) fn log_output(&self) -> Option<PathBuf> {
        self.global
            .as_ref()
            .and_then(|global| global.log_output.as_ref())
            .map(|path| self.resolve_path(path))
    }

    ///
    /// アセットデータ格納ディレクトリのパスへのアクセサ
    ///
    pub(super) fn assets_path(&self) -> Option<PathBuf> {
        self.global
            .as_ref()
            .and_then(|global| global.assets_path.as_ref())
            .map(|path| self.resolve_path(path))
    }

    ///
    /// サブコマンドrunのバインドアドレスへのアクセサ
    ///
    pub(super) fn run_bind_addr(&self) -> Option<String> {
        self.run
            .as_ref()
            .and_then(|run| run.bind_addr.clone())
    }

    ///
    /// サブコマンドrunのバインドポートへのアクセサ
    ///
    pub(super) fn run_bind_port(&self) -> Option<u16> {
        self.run
            .as_ref()
            .and_then(|run| run.bind_port)
    }

    ///
    /// user listサブコマンドのソートモードへのアクセサ
    ///
    pub(super) fn user_list_sort_mode(&self,) -> Option<UserListSortMode> {
        self.user
            .as_ref()
            .and_then(|user| user.list.as_ref())
            .and_then(|list| list.sort_mode)
    }

    ///
    /// user listサブコマンドの逆順指定へのアクセサ
    ///
    pub(super) fn user_list_reverse_sort(&self) -> Option<bool> {
        self.user
            .as_ref()
            .and_then(|user| user.list.as_ref())
            .and_then(|list| list.reverse_sort)
    }

    ///
    /// page listサブコマンドのソートモードへのアクセサ
    ///
    pub(super) fn page_list_sort_mode(&self,) -> Option<PageListSortMode> {
        self.page
            .as_ref()
            .and_then(|page| page.list.as_ref())
            .and_then(|list| list.sort_mode)
    }

    ///
    /// page listサブコマンドの逆順指定へのアクセサ
    ///
    pub(super) fn page_list_reverse_sort(&self) -> Option<bool> {
        self.page
            .as_ref()
            .and_then(|page| page.list.as_ref())
            .and_then(|list| list.reverse_sort)
    }

    ///
    /// page listサブコマンドの詳細表示指定へのアクセサ
    ///
    pub(super) fn page_list_long_info(&self) -> Option<bool> {
        self.page
            .as_ref()
            .and_then(|page| page.list.as_ref())
            .and_then(|list| list.long_info)
    }

    ///
    /// page addサブコマンドのデフォルトユーザ名へのアクセサ
    ///
    pub(super) fn page_add_default_user(&self) -> Option<String> {
        self.page
            .as_ref()
            .and_then(|page| page.add.as_ref())
            .and_then(|add| add.default_user.clone())
    }

    ///
    /// page undeleteサブコマンドのアセット復旧有無へのアクセサ
    ///
    pub(super) fn page_undelete_with_assets(&self) -> Option<bool> {
        self.page
            .as_ref()
            .and_then(|page| page.undelete.as_ref())
            .and_then(|undelete| undelete.with_assets)
    }

    ///
    /// lock listサブコマンドのソートモードへのアクセサ
    ///
    pub(super) fn lock_list_sort_mode(&self,) -> Option<LockListSortMode> {
        self.lock
            .as_ref()
            .and_then(|lock| lock.list.as_ref())
            .and_then(|list| list.sort_mode)
    }

    ///
    /// lock listサブコマンドの逆順指定へのアクセサ
    ///
    pub(super) fn lock_list_reverse_sort(&self) -> Option<bool> {
        self.lock
            .as_ref()
            .and_then(|lock| lock.list.as_ref())
            .and_then(|list| list.reverse_sort)
    }

    ///
    /// lock listサブコマンドの詳細表示指定へのアクセサ
    ///
    pub(super) fn lock_list_long_info(&self) -> Option<bool> {
        self.lock
            .as_ref()
            .and_then(|lock| lock.list.as_ref())
            .and_then(|list| list.long_info)
    }

    ///
    /// asset addサブコマンドのデフォルトユーザ名へのアクセサ
    ///
    pub(super) fn asset_add_default_user(&self) -> Option<String> {
        self.asset
            .as_ref()
            .and_then(|asset| asset.add.as_ref())
            .and_then(|add| add.default_user.clone())
    }

    ///
    /// asset listサブコマンドのソートモードへのアクセサ
    ///
    pub(super) fn asset_list_sort_mode(&self) -> Option<AssetListSortMode> {
        self.asset
            .as_ref()
            .and_then(|asset| asset.list.as_ref())
            .and_then(|list| list.sort_mode)
    }

    ///
    /// asset listサブコマンドの逆順指定へのアクセサ
    ///
    pub(super) fn asset_list_reverse_sort(&self) -> Option<bool> {
        self.asset
            .as_ref()
            .and_then(|asset| asset.list.as_ref())
            .and_then(|list| list.reverse_sort)
    }

    ///
    /// asset listサブコマンドの詳細表示指定へのアクセサ
    ///
    pub(super) fn asset_list_long_info(&self) -> Option<bool> {
        self.asset
            .as_ref()
            .and_then(|asset| asset.list.as_ref())
            .and_then(|list| list.long_info)
    }

    ///
    /// frontend設定へのアクセサ
    ///
    pub(super) fn frontend_config(&self) -> FrontendConfig {
        let default_config = FrontendConfig::default();
        let frontend = self.frontend.as_ref();

        let ui_font = frontend
            .and_then(|section| section.ui_font.clone())
            .unwrap_or_else(|| default_config.ui_font.clone());
        let md_font_sans = frontend
            .and_then(|section| section.md_font_sans.clone())
            .unwrap_or_else(|| default_config.md_font_sans.clone());
        let md_font_serif = frontend
            .and_then(|section| section.md_font_serif.clone())
            .unwrap_or_else(|| default_config.md_font_serif.clone());
        let md_font_mono = frontend
            .and_then(|section| section.md_font_mono.clone())
            .unwrap_or_else(|| default_config.md_font_mono.clone());
        let md_code_font = frontend
            .and_then(|section| section.md_code_font.clone())
            .or_else(|| frontend.and_then(|section| section.md_font_mono.clone()))
            .unwrap_or_else(|| default_config.md_code_font.clone());

        FrontendConfig {
            ui_font,
            md_font_sans,
            md_font_serif,
            md_font_mono,
            md_code_font,
        }
    }

    ///
    /// コンフィギュレーション情報の保存
    ///
    /// # 戻り値
    /// 保存に成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    #[allow(dead_code)]
    pub(super) fn save<P>(&self, path: P) -> Result<()>
    where 
        P: AsRef<Path>
    {
        if let Err(err) = std::fs::write(path, &toml::to_string(self)?) {
            Err(anyhow!("write config error: {}", err))
        } else {
            Ok(())
        }
    }

    ///
    /// グローバル設定の初期化または取得
    ///
    fn ensure_global(&mut self) -> &mut GlobalInfo {
        if self.global.is_none() {
            self.global = Some(GlobalInfo {
                log_level: None,
                log_output: None,
                db_path: None,
                assets_path: None,
            });
        }

        self.global.as_mut().expect("global must be initialized")
    }

    ///
    /// コンフィギュレーションのパスに応じてパスを解決
    ///
    /// # 戻り値
    /// config.tomlが存在するディレクトリを基準に解決したパスを返す。
    ///
    fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return path.to_path_buf();
        }

        if let Some(config_path) = &self.config_path {
            if let Some(parent) = config_path.parent() {
                return Self::normalize_path(parent.join(path));
            }
        }

        path.to_path_buf()
    }

    fn normalize_path(path: PathBuf) -> PathBuf {
        let mut result = PathBuf::new();
        let mut segments: Vec<OsString> = Vec::new();
        let mut prefix: Option<std::path::PrefixComponent<'_>> = None;
        let mut has_root = false;

        for component in path.components() {
            match component {
                Component::Prefix(value) => prefix = Some(value),
                Component::RootDir => has_root = true,
                Component::CurDir => {}
                Component::ParentDir => {
                    if segments.pop().is_none() && !has_root {
                        segments.push(OsString::from(".."));
                    }
                }
                Component::Normal(value) => segments.push(value.to_os_string()),
            }
        }

        if let Some(value) = prefix {
            result.push(value.as_os_str());
        }

        if has_root {
            result.push(Path::new("/"));
        }

        for segment in segments {
            result.push(segment);
        }

        result
    }

    ///
    /// run設定の初期化または取得
    ///
    fn ensure_run(&mut self) -> &mut RunInfo {
        if self.run.is_none() {
            self.run = Some(RunInfo {
                bind_addr: None,
                bind_port: None,
            });
        }

        self.run.as_mut().expect("run must be initialized")
    }

    ///
    /// user list設定の初期化または取得
    ///
    fn ensure_user_list(&mut self) -> &mut UserListInfo {
        if self.user.is_none() {
            self.user = Some(UserSection { list: None });
        }

        let user = self.user.as_mut().expect("user must be initialized");
        if user.list.is_none() {
            user.list = Some(UserListInfo {
                sort_mode: None,
                reverse_sort: None,
            });
        }

        user.list.as_mut().expect("user.list must be initialized")
    }

    ///
    /// page list設定の初期化または取得
    ///
    fn ensure_page_list(&mut self) -> &mut PageListInfo {
        if self.page.is_none() {
            self.page = Some(PageSection {
                list: None,
                add: None,
                undelete: None,
            });
        }

        let page = self.page.as_mut().expect("page must be initialized");
        if page.list.is_none() {
            page.list = Some(PageListInfo {
                sort_mode: None,
                reverse_sort: None,
                long_info: None,
            });
        }

        page.list.as_mut().expect("page.list must be initialized")
    }

    ///
    /// page add設定の初期化または取得
    ///
    fn ensure_page_add(&mut self) -> &mut PageAddInfo {
        if self.page.is_none() {
            self.page = Some(PageSection {
                list: None,
                add: None,
                undelete: None,
            });
        }

        let page = self.page.as_mut().expect("page must be initialized");
        if page.add.is_none() {
            page.add = Some(PageAddInfo {
                default_user: None,
            });
        }

        page.add.as_mut().expect("page.add must be initialized")
    }

    ///
    /// page undelete設定の初期化または取得
    ///
    fn ensure_page_undelete(&mut self) -> &mut PageUndeleteInfo {
        if self.page.is_none() {
            self.page = Some(PageSection {
                list: None,
                add: None,
                undelete: None,
            });
        }

        let page = self.page.as_mut().expect("page must be initialized");
        if page.undelete.is_none() {
            page.undelete = Some(PageUndeleteInfo {
                with_assets: None,
            });
        }

        page.undelete.as_mut().expect("page.undelete must be initialized")
    }
    ///
    /// lock list設定の初期化または取得
    ///
    fn ensure_lock_list(&mut self) -> &mut LockListInfo {
        if self.lock.is_none() {
            self.lock = Some(LockSection { list: None });
        }

        let lock = self.lock.as_mut().expect("lock must be initialized");
        if lock.list.is_none() {
            lock.list = Some(LockListInfo {
                sort_mode: None,
                reverse_sort: None,
                long_info: None,
            });
        }

        lock.list.as_mut().expect("lock.list must be initialized")
    }

    ///
    /// asset add設定の初期化または取得
    ///
    fn ensure_asset_add(&mut self) -> &mut AssetAddInfo {
        if self.asset.is_none() {
            self.asset = Some(AssetSection { add: None, list: None });
        }

        let asset = self.asset.as_mut().expect("asset must be initialized");
        if asset.add.is_none() {
            asset.add = Some(AssetAddInfo {
                default_user: None,
            });
        }

        asset.add.as_mut().expect("asset.add must be initialized")
    }

    ///
    /// asset list設定の初期化または取得
    ///
    fn ensure_asset_list(&mut self) -> &mut AssetListInfo {
        if self.asset.is_none() {
            self.asset = Some(AssetSection { add: None, list: None });
        }

        let asset = self.asset.as_mut().expect("asset must be initialized");
        if asset.list.is_none() {
            asset.list = Some(AssetListInfo {
                sort_mode: None,
                reverse_sort: None,
                long_info: None,
            });
        }

        asset.list.as_mut().expect("asset.list must be initialized")
    }
}

// Defaultトレイトの実装
impl Default for Config {
    fn default() -> Self {
        Self {
            config_path: None,
            global: Some(GlobalInfo {
                log_level: Some(LogLevel::Info),
                log_output: Some(default_log_path()),
                db_path: Some(default_db_path()),
                assets_path: Some(default_assets_path()),
            }),

            run: Some(RunInfo {
                bind_addr: Some("0.0.0.0".to_string()),
                bind_port: Some(8080),
            }),

            user: Some(UserSection {
                list: Some(UserListInfo {
                    sort_mode: Some(UserListSortMode::Default),
                    reverse_sort: Some(false),
                }),
            }),

            page: Some(PageSection {
                add: None,
                undelete: Some(PageUndeleteInfo {
                    with_assets: Some(true),
                }),
                list: Some(PageListInfo {
                    sort_mode: Some(PageListSortMode::Default),
                    reverse_sort: Some(false),
                    long_info: Some(false),
                }),
            }),

            lock: Some(LockSection {
                list: Some(LockListInfo {
                    sort_mode: Some(LockListSortMode::Default),
                    reverse_sort: Some(false),
                    long_info: Some(false),
                }),
            }),

            asset: Some(AssetSection {
                add: None,
                list: Some(AssetListInfo {
                    sort_mode: Some(AssetListSortMode::Default),
                    reverse_sort: Some(false),
                    long_info: Some(false),
                }),
            }),

            frontend: Some(FrontendSection {
                ui_font: Some(DEFAULT_FRONTEND_UI_FONT.to_string()),
                md_font_sans: Some(DEFAULT_FRONTEND_MD_FONT_SANS.to_string()),
                md_font_serif: Some(DEFAULT_FRONTEND_MD_FONT_SERIF.to_string()),
                md_font_mono: Some(DEFAULT_FRONTEND_MD_FONT_MONO.to_string()),
                md_code_font: Some(DEFAULT_FRONTEND_MD_CODE_FONT.to_string()),
            }),
        }
    }
}

///
/// グローバル設定を格納する構造体
///
#[derive(Debug, Deserialize, Serialize)]
struct GlobalInfo {
    /// ログレベル
    log_level: Option<LogLevel>,

    /// ログの出力先
    log_output: Option<PathBuf>,

    /// データベースファイルへのパス
    db_path: Option<PathBuf>,

    /// アセットデータ格納ディレクトリのパス
    assets_path: Option<PathBuf>,
}

///
/// コンフィギュレーション情報の読み込み
///
pub(super) fn load<P>(path: P) -> Result<Config>
where 
    P: AsRef<Path>
{
    let path = path.as_ref();
    let mut config: Config = toml::from_str(&std::fs::read_to_string(path)?)?;
    config.config_path = Some(path.to_path_buf());
    Ok(config)
}

///
/// queryサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct RunInfo {
    /// マッチモード
    bind_addr: Option<String>,

    /// 秘匿項目をマスク表示するか否か
    bind_port: Option<u16>,
}

///
/// userサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct UserSection {
    /// user listサブコマンドの設定情報
    list: Option<UserListInfo>,
}

///
/// pageサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct PageSection {
    /// page listサブコマンドの設定情報
    list: Option<PageListInfo>,

    /// page addサブコマンドの設定情報
    add: Option<PageAddInfo>,

    /// page undeleteサブコマンドの設定情報
    undelete: Option<PageUndeleteInfo>,
}

///
/// lockサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct LockSection {
    /// lock listサブコマンドの設定情報
    list: Option<LockListInfo>,
}

///
/// assetサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct AssetSection {
    /// asset addサブコマンドの設定情報
    add: Option<AssetAddInfo>,

    /// asset listサブコマンドの設定情報
    list: Option<AssetListInfo>,
}

///
/// frontend設定の情報
///
#[derive(Debug, Deserialize, Serialize)]
struct FrontendSection {
    /// UI全体のフォントファミリー
    ui_font: Option<String>,

    /// Markdown表示でのSansフォントファミリー
    md_font_sans: Option<String>,

    /// Markdown表示でのSerifフォントファミリー
    md_font_serif: Option<String>,

    /// Markdown表示でのMonoフォントファミリー
    md_font_mono: Option<String>,

    /// Markdown表示でのコードフォントファミリー
    md_code_font: Option<String>,
}

///
/// frontend設定の解決済みデータ
///
#[derive(Clone, Debug)]
pub(crate) struct FrontendConfig {
    ui_font: String,
    md_font_sans: String,
    md_font_serif: String,
    md_font_mono: String,
    md_code_font: String,
}

impl FrontendConfig {
    pub(crate) fn ui_font(&self) -> &str {
        &self.ui_font
    }

    pub(crate) fn md_font_sans(&self) -> &str {
        &self.md_font_sans
    }

    pub(crate) fn md_font_serif(&self) -> &str {
        &self.md_font_serif
    }

    pub(crate) fn md_font_mono(&self) -> &str {
        &self.md_font_mono
    }

    pub(crate) fn md_code_font(&self) -> &str {
        &self.md_code_font
    }
}

impl Default for FrontendConfig {
    fn default() -> Self {
        Self {
            ui_font: DEFAULT_FRONTEND_UI_FONT.to_string(),
            md_font_sans: DEFAULT_FRONTEND_MD_FONT_SANS.to_string(),
            md_font_serif: DEFAULT_FRONTEND_MD_FONT_SERIF.to_string(),
            md_font_mono: DEFAULT_FRONTEND_MD_FONT_MONO.to_string(),
            md_code_font: DEFAULT_FRONTEND_MD_CODE_FONT.to_string(),
        }
    }
}

///
/// user listサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct UserListInfo {
    /// ソートモード
    sort_mode: Option<UserListSortMode>,

    /// 逆順ソートの有無
    reverse_sort: Option<bool>,
}

///
/// page listサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct PageListInfo {
    /// ソートモード
    sort_mode: Option<PageListSortMode>,

    /// 逆順ソートの有無
    reverse_sort: Option<bool>,

    /// 詳細表示の有無
    long_info: Option<bool>,
}

///
/// page addサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct PageAddInfo {
    /// デフォルトユーザ名
    default_user: Option<String>,
}

///
/// page undeleteサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct PageUndeleteInfo {
    /// アセットを復旧するか否か
    with_assets: Option<bool>,
}

///
/// lock listサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct LockListInfo {
    /// ソートモード
    sort_mode: Option<LockListSortMode>,

    /// 逆順ソートの有無
    reverse_sort: Option<bool>,

    /// 詳細表示の有無
    long_info: Option<bool>,
}

///
/// asset addサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct AssetAddInfo {
    /// デフォルトユーザ名
    default_user: Option<String>,
}

///
/// asset listサブコマンドの設定情報
///
#[derive(Debug, Deserialize, Serialize)]
struct AssetListInfo {
    /// ソートモード
    sort_mode: Option<AssetListSortMode>,

    /// 逆順ソートの有無
    reverse_sort: Option<bool>,

    /// 詳細表示の有無
    long_info: Option<bool>,
}

#[cfg(test)]
mod asset_list_tests {
    use super::*;

    #[test]
    fn load_asset_list_section_from_toml() {
        let toml_str = r#"
            [asset.list]
            sort_mode = "mime_type"
            reverse_sort = true
            long_info = true
        "#;

        let config: Config = toml::from_str(toml_str).expect("parse failed");
        assert_eq!(
            config.asset_list_sort_mode(),
            Some(AssetListSortMode::MimeType)
        );
        assert_eq!(config.asset_list_reverse_sort(), Some(true));
        assert_eq!(config.asset_list_long_info(), Some(true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_user_list_section_from_toml() {
        let toml_str = r#"
            [user.list]
            sort_mode = "user_name"
            reverse_sort = true
        "#;

        let config: Config = toml::from_str(toml_str).expect("parse failed");
        assert_eq!(
            config.user_list_sort_mode(),
            Some(UserListSortMode::UserName)
        );
        assert_eq!(config.user_list_reverse_sort(), Some(true));
    }

    #[test]
    fn default_user_list_values_are_present() {
        let config = Config::default();
        assert_eq!(
            config.user_list_sort_mode(),
            Some(UserListSortMode::Default)
        );
        assert_eq!(config.user_list_reverse_sort(), Some(false));
    }

    #[test]
    fn serialize_uses_user_list_section() {
        let config = Config::default();
        let output = toml::to_string(&config).expect("serialize failed");
        assert!(output.contains("[user.list]"));
        assert!(!output.contains("[user_list]"));
    }

    #[test]
    fn resolve_relative_paths_with_config_dir() {
        let toml_str = r#"
            [global]
            log_output = "log"
            db_path = "./db/database.redb"
            assets_path = "../assets"
        "#;

        let mut config: Config = toml::from_str(toml_str).expect("parse failed");
        config.config_path = Some(PathBuf::from("/tmp/config/config.toml"));

        assert_eq!(
            config.log_output(),
            Some(PathBuf::from("/tmp/config/log"))
        );
        assert_eq!(
            config.db_path(),
            Some(PathBuf::from("/tmp/config/db/database.redb"))
        );
        assert_eq!(
            config.assets_path(),
            Some(PathBuf::from("/tmp/assets"))
        );
    }

    #[test]
    fn preserve_absolute_paths() {
        let toml_str = r#"
            [global]
            log_output = "/var/log/app.log"
            db_path = "/var/db/data.redb"
            assets_path = "/var/data/assets"
        "#;

        let mut config: Config = toml::from_str(toml_str).expect("parse failed");
        config.config_path = Some(PathBuf::from("/tmp/config/config.toml"));

        assert_eq!(
            config.log_output(),
            Some(PathBuf::from("/var/log/app.log"))
        );
        assert_eq!(
            config.db_path(),
            Some(PathBuf::from("/var/db/data.redb"))
        );
        assert_eq!(
            config.assets_path(),
            Some(PathBuf::from("/var/data/assets"))
        );
    }

    #[test]
    fn keep_relative_paths_when_config_path_is_missing() {
        let toml_str = r#"
            [global]
            log_output = "log"
            db_path = "db/database.redb"
            assets_path = "assets"
        "#;

        let config: Config = toml::from_str(toml_str).expect("parse failed");
        assert_eq!(config.log_output(), Some(PathBuf::from("log")));
        assert_eq!(
            config.db_path(),
            Some(PathBuf::from("db/database.redb"))
        );
        assert_eq!(config.assets_path(), Some(PathBuf::from("assets")));
    }
}
